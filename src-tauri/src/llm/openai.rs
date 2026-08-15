use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;

use super::{prompt, ChunkCallback, LlmConfig, LlmProvider, PolishRequest, PolishResponse};

pub struct OpenAiProvider {
    client: Client,
}

/// Wrap a rejected completion so [`crate::retry::is_retryable`] can still see
/// the status — same reasoning as `stt::whisper_compat::upload_error`.
fn api_error(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    let sanitized = crate::retry::truncate_error_body(body);
    crate::retry::HttpStatusError::new(status, format!("LLM API error {status}: {sanitized}"))
        .into()
}

/// GLM series that implement the `thinking` switch.
///
/// GLM-4 predates it: the field is documented for the 4.5 series and later, so
/// sending it to `glm-4-flash-250414` asks a model to toggle a mode it does not
/// have. The old gate was a bare `glm-` prefix, which covered both.
fn glm_supports_thinking(model: &str) -> bool {
    ["glm-4.5", "glm-4.6", "glm-4.7", "glm-5"]
        .iter()
        .any(|series| model.starts_with(series))
}

/// Per-model request fields that control whether the model reasons.
///
/// Polishing a sentence of dictated speech is a formatting task. Thinking buys
/// nothing here and costs plenty: latency on the path between releasing the
/// hotkey and seeing text, tokens against the user's quota, and — when the
/// scratchpad is long enough to exhaust `max_tokens` — the answer itself.
/// Measured against Groq's `qwen/qwen3.6-27b` on "One, two, three": 262
/// completion tokens by default, 7 with reasoning switched off.
///
/// Every entry is gated narrowly, because an unrecognised field is not ignored.
/// Groq answers `property 'x' is unsupported` with a 400, and
/// `reasoning_effort: "none"` is valid for Qwen3 but rejected by GPT-OSS, which
/// takes only `low` / `medium` / `high`. So there is no safe "send it
/// everywhere, the ones that don't care will drop it" option, and widening a
/// gate needs a probe rather than an assumption. Whatever reasons anyway is
/// caught by [`crate::llm::think`], which is the layer that has to hold for
/// providers this list has never heard of.
///
/// Probed against the live API: the Gemini, Groq/Qwen3 and Groq/GPT-OSS rows.
/// The DeepSeek, Moonshot and Zhipu rows come from the vendors' published
/// request schemas and have **not** been probed — no key to hand. Each is a
/// field those schemas enumerate for that exact model, and the failure mode if
/// one is wrong is a 400 that surfaces as "Polish: provider error" with the raw
/// transcript pasted, not silent corruption.
fn reasoning_params(model: &str, base_url: &str) -> Vec<(&'static str, serde_json::Value)> {
    let mut params = Vec::new();

    // Gemini 3.x flash-lite has thinking off by default; the explicit opt-out is
    // defensive against a future model where it engages.
    if model.contains("gemini") {
        params.push(("reasoning_effort", serde_json::json!("none")));
    }

    // Qwen3 reasons by default and, on Groq, reports it as `raw` — the
    // scratchpad lands in `content`. Groq is the only endpoint confirmed to
    // accept `reasoning_effort: "none"` for it; Alibaba's DashScope spells the
    // same switch `enable_thinking`, so a bare `qwen3` match would break there.
    if model.contains("qwen3") && base_url.contains("groq.com") {
        params.push(("reasoning_effort", serde_json::json!("none")));
    }

    // DeepSeek V4 merged reasoning into every model as a request flag, defaulting
    // to enabled. Kimi K2.5/K2.6 use the identical shape; K2.7-code rejects
    // `disabled` and K3 has no off switch at all, so both stay out of the gate.
    let deepseek_v4 = model.starts_with("deepseek-v4") && base_url.contains("deepseek.com");
    let kimi_k25 = matches!(model, "kimi-k2.5" | "kimi-k2.6") && base_url.contains("moonshot.");
    if deepseek_v4 || kimi_k25 {
        params.push(("thinking", serde_json::json!({ "type": "disabled" })));
    }

    // GLM is the one place this app turns thinking *on*. Left off, the API
    // populates `reasoning_content` and leaves `content` empty, so the polish
    // came back blank; enabling it makes both fields arrive. Thinking mode also
    // requires temperature >= 0.6, hence the override.
    if glm_supports_thinking(model) {
        params.push(("thinking", serde_json::json!({ "type": "enabled" })));
        params.push(("temperature", serde_json::json!(1.0)));
        params.push(("top_p", serde_json::json!(0.95)));
    }

    params
}

impl OpenAiProvider {
    /// Takes the app-wide pooled client (`crate::HttpClient`) rather than
    /// building one, so polish requests reuse a warm connection — and so this
    /// provider cannot quietly opt out of the pool.
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn polish(
        &self,
        config: &LlmConfig,
        req: &PolishRequest,
        on_chunk: Option<&ChunkCallback>,
    ) -> Result<PolishResponse> {
        let has_selected_text = req
            .selected_text
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());

        let system_prompt = prompt::build_system_prompt(
            req.app_type,
            &req.dictionary,
            req.translate_enabled,
            &req.target_lang,
            has_selected_text,
            req.detected_language.as_deref(),
            &req.user_languages,
        );

        let mut messages = vec![serde_json::json!({ "role": "system", "content": system_prompt })];
        if has_selected_text {
            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("<selected_text>\n{}\n</selected_text>", req.selected_text.as_ref().unwrap())
            }));
        }
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!("<transcription>\n{}\n</transcription>", req.raw_text)
        }));

        let mut body = serde_json::json!({
            "model": config.model,
            "messages": messages,
            "max_tokens": config.max_tokens,
            "temperature": config.temperature,
            "stream": on_chunk.is_some()
        });

        if let Some(obj) = body.as_object_mut() {
            for (k, v) in reasoning_params(&config.model, &config.base_url) {
                obj.insert(k.to_string(), v);
            }
        }

        // Retry covers the request head only. Once the body starts streaming,
        // chunks have already reached the callback and the frontend has drawn
        // them, so a second attempt would duplicate visible text — see
        // `crate::retry`.
        let response = crate::retry::with_retry("LLM polish", || async {
            let response = self
                .client
                .post(format!("{}/chat/completions", config.base_url))
                .header("Authorization", format!("Bearer {}", config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(60))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(api_error(status, &text));
            }

            Ok(response)
        })
        .await?;

        if let Some(callback) = on_chunk {
            // Streaming mode
            let mut full_text = String::new();
            let mut reasoning_text = String::new();
            // Chunks reach the frontend as they arrive, so a `<think>` block has
            // to be caught here rather than trimmed off the finished string —
            // by then the user has already watched it appear.
            let mut think = super::think::ThinkFilter::new();
            let mut stream = response.bytes_stream();

            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process SSE lines
                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            break;
                        }
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                            let delta = &v["choices"][0]["delta"];

                            if let Some(content) = delta["content"].as_str() {
                                if !content.is_empty() {
                                    let visible = think.push(content);
                                    if !visible.is_empty() {
                                        full_text.push_str(&visible);
                                        callback(&visible);
                                    }
                                }
                            }

                            // Collect reasoning_content as fallback for thinking-mode models
                            // where all output may land in this field instead of content
                            if let Some(rc) = delta["reasoning_content"].as_str() {
                                if !rc.is_empty() {
                                    reasoning_text.push_str(rc);
                                }
                            }
                        }
                    }
                }
            }

            // Anything held back waiting to find out whether it was a tag.
            let tail = think.finish();
            if !tail.is_empty() {
                full_text.push_str(&tail);
                callback(&tail);
            }

            // A response that was nothing but an unterminated scratchpad has no
            // answer in it — `max_tokens` cut it off mid-thought. Failing is the
            // useful outcome: the pipeline then pastes the raw transcript rather
            // than typing an empty string, and leaves a selection untouched.
            if full_text.trim().is_empty() && think.truncated_in_reasoning() {
                return Err(anyhow::anyhow!(
                    "LLM spent the whole {} token budget on reasoning and never produced an answer",
                    config.max_tokens
                ));
            }

            // If content was empty but reasoning_content had text, use it as output.
            // This handles GLM thinking-mode where the API puts all output in reasoning_content.
            //
            // Only reachable when nothing arrived on `content` at all: a model that
            // reports reasoning in *both* fields has already had the `<think>` block
            // dropped above, and its answer is in `full_text`.
            if full_text.is_empty() && !reasoning_text.is_empty() {
                tracing::warn!(
                    "LLM content empty, using reasoning_content ({} chars) as output",
                    reasoning_text.len()
                );
                callback(&reasoning_text);
                full_text = reasoning_text;
            } else if full_text.is_empty() {
                tracing::error!("LLM streaming returned no content and no reasoning_content");
            }

            Ok(PolishResponse {
                polished_text: full_text,
            })
        } else {
            // Non-streaming mode
            let v: serde_json::Value = response.json().await?;
            let text =
                super::think::strip(v["choices"][0]["message"]["content"].as_str().unwrap_or(""));

            if text.is_empty() {
                tracing::warn!(
                    "LLM non-streaming returned empty content, full response: {}",
                    v
                );
            }

            Ok(PolishResponse {
                polished_text: text,
            })
        }
    }

    fn name(&self) -> &str {
        "OpenAI"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_is_retryable_for_rate_limits_and_server_errors() {
        for status in [
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::BAD_GATEWAY,
        ] {
            let err = api_error(status, "try later");
            assert!(
                crate::retry::is_retryable(&err),
                "{status} must stay retryable so a blip doesn't drop the polish"
            );
        }
    }

    const GROQ: &str = "https://api.groq.com/openai/v1";
    const ZHIPU: &str = "https://open.bigmodel.cn/api/paas/v4";

    fn effort(model: &str, base_url: &str) -> Option<String> {
        reasoning_params(model, base_url)
            .into_iter()
            .find(|(k, _)| *k == "reasoning_effort")
            .map(|(_, v)| v.as_str().unwrap_or_default().to_string())
    }

    /// The reported failure: Groq serves Qwen3 with raw reasoning on by default,
    /// so the scratchpad arrived in `content` and was typed into the document.
    #[test]
    fn qwen3_on_groq_asks_for_no_reasoning() {
        assert_eq!(effort("qwen/qwen3.6-27b", GROQ).as_deref(), Some("none"));
    }

    /// DashScope spells the same switch `enable_thinking` and Groq rejects a
    /// field a model does not know, so the gate stays on the endpoint we probed.
    #[test]
    fn qwen3_elsewhere_is_left_alone() {
        assert!(effort(
            "qwen3-32b",
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        )
        .is_none());
    }

    /// GPT-OSS takes only low/medium/high — "none" is a 400. It reports its
    /// reasoning in a separate field anyway, so `content` was never polluted.
    #[test]
    fn gpt_oss_gets_no_reasoning_effort() {
        assert!(effort("openai/gpt-oss-120b", GROQ).is_none());
    }

    #[test]
    fn gemini_keeps_its_existing_opt_out() {
        assert_eq!(
            effort(
                "gemini-2.5-flash-lite",
                "https://generativelanguage.googleapis.com/v1beta/openai"
            )
            .as_deref(),
            Some("none")
        );
    }

    /// A model with no known switch must send nothing at all rather than a
    /// guess, or the request fails outright on a strict provider.
    #[test]
    fn unknown_models_send_no_reasoning_fields() {
        for model in ["gpt-4o-mini", "deepseek-chat", "llama-3.3-70b-versatile"] {
            assert!(
                reasoning_params(model, GROQ).is_empty(),
                "{model} must not carry an unprobed parameter"
            );
        }
    }

    fn thinking(model: &str, base_url: &str) -> Option<serde_json::Value> {
        reasoning_params(model, base_url)
            .into_iter()
            .find(|(k, _)| *k == "thinking")
            .map(|(_, v)| v)
    }

    /// GLM-4 predates the `thinking` switch entirely, and `glm-4-flash-250414`
    /// is the shipped Zhipu default — the old bare `glm-` gate sent it a field
    /// its API does not define.
    #[test]
    fn glm_4_generation_is_not_asked_to_toggle_thinking() {
        for model in ["glm-4-flash-250414", "glm-4-32b-0414-128k", "glm-4-plus"] {
            assert!(
                reasoning_params(model, ZHIPU).is_empty(),
                "{model} has no thinking mode to configure"
            );
        }
    }

    /// The 4.5-and-later series still need thinking switched on, or `content`
    /// comes back empty and the polish is blank.
    #[test]
    fn glm_45_and_later_still_enable_thinking() {
        for model in ["glm-4.5-flash", "glm-4.7-flash", "glm-5.2"] {
            assert_eq!(
                thinking(model, ZHIPU),
                Some(serde_json::json!({ "type": "enabled" })),
                "{model} must keep the enable-thinking workaround"
            );
        }
    }

    #[test]
    fn deepseek_v4_and_kimi_k25_switch_thinking_off() {
        assert_eq!(
            thinking("deepseek-v4-flash", "https://api.deepseek.com/v1"),
            Some(serde_json::json!({ "type": "disabled" }))
        );
        assert_eq!(
            thinking("kimi-k2.5", "https://api.moonshot.cn/v1"),
            Some(serde_json::json!({ "type": "disabled" }))
        );
    }

    /// `kimi-k2.7-code` errors on `disabled` and `kimi-k3` has no off switch, so
    /// neither may be swept in by a looser prefix match.
    #[test]
    fn kimi_models_without_an_off_switch_are_left_alone() {
        for model in ["kimi-k2.7-code", "kimi-k3"] {
            assert!(
                thinking(model, "https://api.moonshot.cn/v1").is_none(),
                "{model} rejects or ignores an explicit disable"
            );
        }
    }

    #[test]
    fn api_error_is_fatal_for_a_rejected_key() {
        let err = api_error(reqwest::StatusCode::UNAUTHORIZED, "invalid api key");
        assert!(!crate::retry::is_retryable(&err));
        assert_eq!(
            err.to_string(),
            "LLM API error 401 Unauthorized: invalid api key"
        );
    }
}

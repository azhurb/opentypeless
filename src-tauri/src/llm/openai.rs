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

        // GLM-4.7/4.5/5 default to thinking mode, but without explicitly enabling it
        // the API may return content in reasoning_content only, leaving content empty.
        // Explicitly enable thinking so both fields are properly populated.
        // Thinking mode also requires temperature >= 0.6 (recommended 1.0).
        if config.model.starts_with("glm-") {
            if let Some(obj) = body.as_object_mut() {
                obj.insert(
                    "thinking".to_string(),
                    serde_json::json!({"type": "enabled"}),
                );
                obj.insert("temperature".to_string(), serde_json::json!(1.0));
                obj.insert("top_p".to_string(), serde_json::json!(0.95));
            }
        }

        // Gemini 3.x flash-lite has thinking OFF by default, but explicitly opt out so it
        // never engages — voice-to-text polishing is a low-stakes formatting task, not a
        // reasoning task, and any thinking budget would only add latency and cost.
        // Other providers silently ignore the field.
        if config.model.contains("gemini") {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("reasoning_effort".to_string(), serde_json::json!("none"));
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
                                    full_text.push_str(content);
                                    callback(content);
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

            // If content was empty but reasoning_content had text, use it as output.
            // This handles GLM thinking-mode where the API puts all output in reasoning_content.
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
            let text = v["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();

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

use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;

use super::{DisconnectResult, SttConfig, SttProvider, TranscriptEvent};

pub const PROVIDER_NAME: &str = "Gemini Transcribe";
pub const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";
pub const MODEL: &str = "gemini-3.5-transcribe";

/// Max audio buffer: ~24 MB PCM ≈ 12.5 min at 16kHz 16-bit mono.
///
/// The binding limit here is the Interactions API's 100 MB per-request ceiling
/// for inline audio, and base64 inflates by 4/3 — so 24 MB of PCM becomes a
/// ~32 MB body, comfortably inside it. The cap is kept identical to
/// [`super::whisper_compat`]'s so switching provider never changes how long a
/// dictation may run.
const MAX_AUDIO_BYTES: usize = 24 * 1024 * 1024;

/// Gemini accepts up to 1,000 custom-vocabulary terms and documents that best
/// results come from ~100. We send the API's ceiling rather than the advice:
/// truncating someone's dictionary at 100 silently drops words they added on
/// purpose, and the failure mode past that point is degraded biasing, not an
/// error.
const MAX_VOCABULARY_TERMS: usize = 1000;

/// Map an ISO-639-1 code from `SttConfig.languages` to the BCP-47 tag the
/// transcription API expects (`language_codes` is documented as region-tagged:
/// `en-US`, `es-ES`).
///
/// Our Settings UI only offers bare ISO-639-1 codes, so the region has to be
/// chosen here. Where a language has more than one plausible region we take the
/// larger speaker population (`pt-BR`, not `pt-PT`) or the standard written
/// form (`ar-SA` for Modern Standard Arabic, `zh-CN` for simplified). A user who
/// needs `en-GB` spelling cannot express it today — see the follow-up in
/// `docs/plans/active/gemini-transcribe.md`.
///
/// An unrecognized code returns `None` and is dropped from the request. This is
/// tidiness, not safety: the API was measured on 2026-08-27 to accept a bare
/// `en` and even a gibberish `xx-YY` with a 200, so an unmapped code passed
/// through would be ignored rather than rejected. Dropping it keeps the request
/// to the documented shape and makes the mapping table the single place that
/// decides what we claim to support.
fn bcp47(code: &str) -> Option<&'static str> {
    Some(match code.trim().to_lowercase().as_str() {
        "zh" => "zh-CN",
        "en" => "en-US",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "fr" => "fr-FR",
        "de" => "de-DE",
        "es" => "es-ES",
        "pt" => "pt-BR",
        "ru" => "ru-RU",
        "ar" => "ar-SA",
        "hi" => "hi-IN",
        "th" => "th-TH",
        "vi" => "vi-VN",
        "it" => "it-IT",
        "nl" => "nl-NL",
        "tr" => "tr-TR",
        "pl" => "pl-PL",
        "uk" => "uk-UA",
        "id" => "id-ID",
        "ms" => "ms-MY",
        _ => return None,
    })
}

/// Build the Interactions API request body. Pure so the wire shape is
/// unit-testable without a socket.
///
/// Unlike the Whisper-compatible providers, `language_codes` is a *set*: the
/// user's whole selection goes on the wire and the model handles code-switching
/// between them. An empty set omits the field, which the API documents as
/// auto-detect.
///
/// `smart` maps `SttConfig.smart_format` onto the documented transcription
/// modes: `smart` strips fillers and false starts and formats spoken lists,
/// dates and numbers; `verbatim` is the API default and transcribes literally.
///
/// Diarization and word-level timestamps are deliberately not requested. A
/// dictation is one speaker and the text is typed straight into the foreground
/// app, so neither is used downstream, and enabling either cuts the accepted
/// audio length from 60 to 30 minutes for nothing.
fn build_request_body(
    languages: &[String],
    smart: bool,
    vocabulary: &[String],
    audio_b64: String,
) -> serde_json::Value {
    let mut transcription_config = serde_json::Map::new();

    let codes: Vec<&str> = languages.iter().filter_map(|c| bcp47(c)).collect();
    if !codes.is_empty() {
        transcription_config.insert("language_codes".into(), serde_json::json!(codes));
    }

    if !vocabulary.is_empty() {
        let terms: Vec<&String> = vocabulary.iter().take(MAX_VOCABULARY_TERMS).collect();
        transcription_config.insert("custom_vocabulary".into(), serde_json::json!(terms));
    }

    transcription_config.insert(
        "mode".into(),
        serde_json::json!({ "type": if smart { "smart" } else { "verbatim" } }),
    );

    serde_json::json!({
        "model": MODEL,
        "input": [{
            "type": "audio",
            "data": audio_b64,
            "mime_type": "audio/wav",
        }],
        "generation_config": {
            "transcription_config": serde_json::Value::Object(transcription_config),
        },
    })
}

/// Pull the transcript out of an Interactions API response.
///
/// **`steps[].content[].text` is where the transcript actually is.** The docs
/// name `interaction.output_text`, but that is the SDK object's accessor, not a
/// REST field: a live response (2026-08-27) has top-level keys `id`, `status`,
/// `usage`, `created`, `updated`, `service_tier`, `steps`, `object`, `model`
/// and no `output_text` at all. It is still checked first, so a future REST
/// version that does expose it wins without a code change.
///
/// Content items are typed and only `text` is collected. The response carries
/// `{"type": "text", ...}` inside a `{"type": "model_output"}` step today, and
/// `usage` counts `total_thought_tokens` and `total_tool_use_tokens` separately
/// — so a step type that is not model output is a real possibility. Collecting
/// every `text` field regardless of type is how a reasoning scratchpad ends up
/// typed into the user's document, which is exactly the 0.8.0 `<think>` bug.
fn parse_response(body: &str) -> Result<String> {
    let v: serde_json::Value = serde_json::from_str(body)?;

    if let Some(text) = v["output_text"].as_str() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let mut parts: Vec<&str> = Vec::new();
    if let Some(steps) = v["steps"].as_array() {
        for step in steps {
            if let Some(content) = step["content"].as_array() {
                for item in content {
                    if item["type"] != "text" {
                        continue;
                    }
                    if let Some(text) = item["text"].as_str() {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            parts.push(trimmed);
                        }
                    }
                }
            }
        }
    }

    Ok(parts.join(" "))
}

/// Wrap a rejected request so [`crate::retry::is_retryable`] can still see the
/// status. A plain `bail!` erases it, which silently turns every transient 503
/// back into a lost dictation — hence the dedicated helper and its tests.
fn api_error(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    let sanitized = crate::retry::truncate_error_body(body);
    tracing::error!("{} HTTP {}: {}", PROVIDER_NAME, status, sanitized);
    crate::retry::HttpStatusError::new(
        status,
        format!("{PROVIDER_NAME} error ({status}): {sanitized}"),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(languages: &[&str], smart: bool, vocabulary: &[&str]) -> serde_json::Value {
        let langs: Vec<String> = languages.iter().map(|s| s.to_string()).collect();
        let vocab: Vec<String> = vocabulary.iter().map(|s| s.to_string()).collect();
        build_request_body(&langs, smart, &vocab, "AAAA".to_string())
    }

    fn transcription_config(v: &serde_json::Value) -> &serde_json::Value {
        &v["generation_config"]["transcription_config"]
    }

    #[test]
    fn api_error_is_retryable_for_server_errors() {
        let err = api_error(reqwest::StatusCode::SERVICE_UNAVAILABLE, "busy");
        assert!(
            crate::retry::is_retryable(&err),
            "a 503 must stay retryable, or a transient blip loses the dictation"
        );
        assert_eq!(
            err.to_string(),
            "Gemini Transcribe error (503 Service Unavailable): busy"
        );
    }

    #[test]
    fn api_error_is_fatal_for_a_rejected_key() {
        let err = api_error(reqwest::StatusCode::UNAUTHORIZED, "nope");
        assert!(
            !crate::retry::is_retryable(&err),
            "a rejected key must reach the user immediately"
        );
    }

    #[test]
    fn body_sends_audio_inline_as_base64_wav() {
        let v = body(&[], true, &[]);
        let input = &v["input"][0];
        assert_eq!(input["type"], "audio");
        assert_eq!(input["mime_type"], "audio/wav");
        assert_eq!(
            input["data"], "AAAA",
            "audio must ride inline in the request; a Files API URI would cost an extra round-trip \
             between hotkey release and text appearing"
        );
        assert!(
            input.get("uri").is_none(),
            "inline and uri are alternatives, not both"
        );
    }

    #[test]
    fn body_omits_language_codes_when_set_is_empty() {
        let v = body(&[], true, &[]);
        assert!(
            transcription_config(&v).get("language_codes").is_none(),
            "an empty selection must omit the field so the model auto-detects"
        );
    }

    #[test]
    fn body_maps_iso_codes_to_region_tagged_bcp47() {
        let v = body(&["de"], true, &[]);
        assert_eq!(
            transcription_config(&v)["language_codes"],
            serde_json::json!(["de-DE"])
        );
    }

    #[test]
    fn body_carries_every_selected_language() {
        let v = body(&["en", "de", "uk"], true, &[]);
        assert_eq!(
            transcription_config(&v)["language_codes"],
            serde_json::json!(["en-US", "de-DE", "uk-UA"]),
            "unlike the Whisper API this one takes a set, so a multi-language selection reaches \
             the wire instead of being dropped to auto-detect"
        );
    }

    #[test]
    fn body_drops_unknown_language_codes() {
        let v = body(&["en", "klingon"], true, &[]);
        assert_eq!(
            transcription_config(&v)["language_codes"],
            serde_json::json!(["en-US"]),
            "an unmappable code is dropped so the request keeps the documented shape; the API \
             ignores unknown tags rather than rejecting them, so this is tidiness, not safety"
        );
    }

    #[test]
    fn body_omits_language_codes_when_nothing_maps() {
        let v = body(&["klingon"], true, &[]);
        assert!(
            transcription_config(&v).get("language_codes").is_none(),
            "dropping every code must leave the field absent, not an empty array"
        );
    }

    #[test]
    fn smart_format_selects_smart_mode() {
        let v = body(&[], true, &[]);
        assert_eq!(transcription_config(&v)["mode"]["type"], "smart");
    }

    #[test]
    fn smart_format_off_selects_verbatim_mode() {
        let v = body(&[], false, &[]);
        assert_eq!(transcription_config(&v)["mode"]["type"], "verbatim");
    }

    #[test]
    fn body_requests_no_diarization_or_word_timestamps() {
        let v = body(&[], true, &[]);
        let mode = &transcription_config(&v)["mode"];
        assert!(
            mode.get("diarization_mode").is_none() && mode.get("timestamp_granularities").is_none(),
            "a dictation is one speaker and nothing downstream reads timestamps; asking for either \
             halves the accepted audio length for no gain"
        );
    }

    #[test]
    fn body_omits_custom_vocabulary_when_the_dictionary_is_empty() {
        let v = body(&[], true, &[]);
        assert!(transcription_config(&v).get("custom_vocabulary").is_none());
    }

    #[test]
    fn body_sends_dictionary_terms_as_custom_vocabulary() {
        let v = body(&[], true, &["Akeneo", "OpenTypeless"]);
        assert_eq!(
            transcription_config(&v)["custom_vocabulary"],
            serde_json::json!(["Akeneo", "OpenTypeless"])
        );
    }

    #[test]
    fn body_truncates_vocabulary_at_the_api_ceiling() {
        let terms: Vec<String> = (0..MAX_VOCABULARY_TERMS + 50)
            .map(|i| format!("term{i}"))
            .collect();
        let v = build_request_body(&[], true, &terms, "AAAA".to_string());
        assert_eq!(
            transcription_config(&v)["custom_vocabulary"]
                .as_array()
                .unwrap()
                .len(),
            MAX_VOCABULARY_TERMS,
            "an oversized dictionary must be trimmed here, not rejected by the API mid-dictation"
        );
    }

    #[test]
    fn parse_response_reads_output_text() {
        let body = r#"{"output_text": "Hello world"}"#;
        assert_eq!(parse_response(body).unwrap(), "Hello world");
    }

    #[test]
    fn parse_response_trims_output_text() {
        let body = r#"{"output_text": "  spaced  "}"#;
        assert_eq!(parse_response(body).unwrap(), "spaced");
    }

    #[test]
    fn parse_response_reads_the_shape_the_live_api_returns() {
        // Trimmed from a real 2026-08-27 response: no `output_text` anywhere,
        // transcript inside a typed content item of a `model_output` step.
        let body = r#"{
            "id": "v1_abc",
            "status": "completed",
            "object": "interaction",
            "model": "gemini-3.5-transcribe",
            "steps": [
                {"type": "model_output", "content": [
                    {"type": "text", "text": "Please push the changes to the main branch."}
                ]}
            ]
        }"#;
        assert_eq!(
            parse_response(body).unwrap(),
            "Please push the changes to the main branch.",
            "this is the only path that fires against the live API; if it breaks, every \
             dictation reads as silence"
        );
    }

    #[test]
    fn parse_response_joins_multiple_content_items() {
        let body = r#"{
            "steps": [
                {"type": "model_output", "content": [
                    {"type": "text", "text": "Hello"}, {"type": "text", "text": "world"}
                ]},
                {"type": "model_output", "content": [{"type": "text", "text": "again"}]}
            ]
        }"#;
        assert_eq!(parse_response(body).unwrap(), "Hello world again");
    }

    #[test]
    fn parse_response_skips_content_that_is_not_text() {
        let body = r#"{
            "steps": [
                {"type": "thought", "content": [
                    {"type": "thinking", "text": "The user probably means the main branch."}
                ]},
                {"type": "model_output", "content": [
                    {"type": "text", "text": "Push to main."}
                ]}
            ]
        }"#;
        assert_eq!(
            parse_response(body).unwrap(),
            "Push to main.",
            "collecting every text field regardless of type is how a reasoning scratchpad gets \
             typed into the user's document — the 0.8.0 <think> bug in a new place"
        );
    }

    #[test]
    fn parse_response_prefers_output_text_over_steps() {
        let body = r#"{
            "output_text": "the whole thing",
            "steps": [{"type": "model_output", "content": [{"type": "text", "text": "a fragment"}]}]
        }"#;
        assert_eq!(parse_response(body).unwrap(), "the whole thing");
    }

    #[test]
    fn parse_response_returns_empty_for_silence() {
        let body = r#"{"output_text": ""}"#;
        assert_eq!(
            parse_response(body).unwrap(),
            "",
            "an empty transcript is a real outcome, not a parse failure"
        );
    }

    #[test]
    fn parse_response_errors_on_malformed_json() {
        assert!(parse_response("not json").is_err());
    }

    /// End-to-end against the live API. Ignored by default: it needs a key and a
    /// network, so it can never run in CI (see `docs/references/commands.md` —
    /// CI has no secrets and this fork's workflows are manual anyway).
    ///
    /// This is the only check that exercises the real request path rather than a
    /// hand-copied approximation of it, so run it after touching the wire shape:
    ///
    /// ```text
    /// GEMINI_API_KEY=... cargo test --manifest-path src-tauri/Cargo.toml \
    ///     --lib stt::gemini::tests::live -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "requires GEMINI_API_KEY and network"]
    async fn live_round_trips_against_the_real_api() {
        let api_key = match std::env::var("GEMINI_API_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => panic!("set GEMINI_API_KEY to run this test"),
        };

        // Default payload is a second of silence: enough to prove the request is
        // accepted and the response parses, and it ships in the repo as nothing
        // at all. Point `GEMINI_TEST_WAV` at a 16 kHz 16-bit mono WAV to run the
        // same path over real speech and see the transcript on stdout — that is
        // what exercises the `steps[].content[].text` walk, which silence never
        // reaches.
        let pcm = match std::env::var("GEMINI_TEST_WAV") {
            Ok(path) if !path.is_empty() => {
                let bytes = std::fs::read(&path).expect("GEMINI_TEST_WAV must be readable");
                // Skip the 44-byte canonical WAV header: `send_audio` takes raw PCM.
                bytes[44..].to_vec()
            }
            _ => vec![0u8; 32000],
        };
        let mut provider = GeminiTranscribeProvider::new(reqwest::Client::new());
        let config = SttConfig {
            api_key,
            languages: vec!["en".to_string()],
            smart_format: true,
            sample_rate: 16000,
            custom_vocabulary: vec!["OpenTypeless".to_string()],
        };

        provider.connect(&config).await.expect("connect");
        provider.send_audio(&pcm).await.expect("send_audio");
        let result = provider
            .disconnect()
            .await
            .expect("the API must accept our request shape");

        // Silence legitimately transcribes to nothing, which arrives as `None`.
        println!("live result: {result:?}");
    }
}

/// Batch (file-based) provider for `gemini-3.5-transcribe` over the Interactions
/// API.
///
/// Audio is buffered for the length of the dictation and sent as one request on
/// `disconnect`, the same shape as the Whisper-compatible providers. The bytes
/// go inline as base64 rather than through the Files API: an upload-then-
/// reference sequence would put an extra round-trip between the user releasing
/// the hotkey and text appearing, and the Interactions API accepts up to 100 MB
/// of inline payload, far more than a dictation produces.
///
/// For real-time partials, `gemini-3.5-transcribe-live` over the Live API is the
/// streaming counterpart and would be a separate provider in the shape of
/// [`super::deepgram`].
pub struct GeminiTranscribeProvider {
    stt_config: Option<SttConfig>,
    audio_buffer: Vec<u8>,
    client: reqwest::Client,
}

impl GeminiTranscribeProvider {
    /// Takes the app-wide pooled client (`crate::HttpClient`) rather than
    /// building one, so requests reuse a warm connection.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            stt_config: None,
            audio_buffer: Vec::new(),
            client,
        }
    }
}

#[async_trait]
impl SttProvider for GeminiTranscribeProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<()> {
        if config.api_key.is_empty() {
            anyhow::bail!("{PROVIDER_NAME} API key is empty");
        }
        self.stt_config = Some(config.clone());
        self.audio_buffer.clear();
        tracing::info!("{PROVIDER_NAME} provider ready (buffering mode)");
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<()> {
        if self.audio_buffer.len() + chunk.len() > MAX_AUDIO_BYTES {
            anyhow::bail!("{PROVIDER_NAME}: audio exceeds maximum length (~12 min)");
        }
        self.audio_buffer.extend_from_slice(chunk);
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>> {
        // File-based — transcription happens in disconnect().
        Ok(None)
    }

    async fn disconnect(&mut self) -> Result<DisconnectResult> {
        let config = match &self.stt_config {
            Some(c) => c.clone(),
            None => return Ok(None),
        };

        if self.audio_buffer.is_empty() {
            tracing::info!("{PROVIDER_NAME}: no audio buffered, skipping");
            return Ok(None);
        }

        let audio_len_secs = self.audio_buffer.len() as f64 / (config.sample_rate as f64 * 2.0);
        let wav = super::whisper_compat::WhisperCompatProvider::build_wav(
            &self.audio_buffer,
            config.sample_rate,
        );
        self.audio_buffer.clear();

        if config.custom_vocabulary.len() > MAX_VOCABULARY_TERMS {
            tracing::warn!(
                "{}: dictionary has {} terms, sending the first {}",
                PROVIDER_NAME,
                config.custom_vocabulary.len(),
                MAX_VOCABULARY_TERMS
            );
        }

        let audio_b64 = base64::engine::general_purpose::STANDARD.encode(&wav);
        tracing::info!(
            "{}: sending {:.1}s of audio for transcription ({} vocabulary terms)",
            PROVIDER_NAME,
            audio_len_secs,
            config.custom_vocabulary.len().min(MAX_VOCABULARY_TERMS)
        );

        // Safe to retry: one idempotent POST, and nothing has been shown to the
        // user yet. A 429 or 502 here would otherwise throw away the whole
        // utterance the user just spoke.
        let text = crate::retry::with_retry(&format!("{PROVIDER_NAME} transcription"), || async {
            let body = build_request_body(
                &config.languages,
                config.smart_format,
                &config.custom_vocabulary,
                audio_b64.clone(),
            );

            let resp = self
                .client
                .post(ENDPOINT)
                .header("x-goog-api-key", &config.api_key)
                .json(&body)
                .timeout(std::time::Duration::from_secs(60))
                .send()
                .await?;

            let status = resp.status();
            let body = resp.text().await?;

            if !status.is_success() {
                return Err(api_error(status, &body));
            }

            parse_response(&body)
        })
        .await?;

        tracing::info!("{} transcription: {} chars", PROVIDER_NAME, text.len());

        if text.is_empty() {
            Ok(None)
        } else {
            // The API documents automatic language identification but not a
            // response field that reports which language it settled on, so no
            // detected-language badge is shown — the same as AssemblyAI.
            Ok(Some((text, None)))
        }
    }

    fn name(&self) -> &str {
        PROVIDER_NAME
    }
}

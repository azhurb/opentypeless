use anyhow::Result;
use async_trait::async_trait;

use super::{DisconnectResult, SttConfig, SttProvider, TranscriptEvent};

/// Build the multipart text fields for a Whisper-compatible transcription
/// request. Pure helper so the form construction is unit-testable.
///
/// The `languages` rule mirrors the Whisper API's wire constraint: the API
/// takes at most one ISO-639-1 hint or none (auto). We therefore pin only
/// when the user has selected exactly one language; an empty or multi-element
/// set omits the field so the provider auto-detects.
fn build_form_text_fields(
    model: &str,
    languages: &[String],
    extra: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut fields = Vec::with_capacity(2 + extra.len() + 1);
    fields.push(("model".to_string(), model.to_string()));
    fields.push(("response_format".to_string(), "verbose_json".to_string()));
    if languages.len() == 1 {
        fields.push(("language".to_string(), languages[0].clone()));
    }
    for &(k, v) in extra {
        fields.push((k.to_string(), v.to_string()));
    }
    fields
}

/// Parse a Whisper-compatible `verbose_json` response into
/// `(trimmed_text, detected_language_code)`. The language field is optional —
/// some providers omit it for `verbose_json`; if absent or unrecognized we
/// return `None` rather than failing.
fn parse_response(body: &str) -> Result<(String, Option<String>)> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    let text = v["text"].as_str().unwrap_or("").trim().to_string();
    let lang = v["language"].as_str().and_then(normalize_language);
    Ok((text, lang))
}

/// Map a `verbose_json` `language` value (which providers return as either a
/// two-letter ISO-639-1 code or a lowercase English name like `"english"`)
/// to the short codes we use internally. Returns `None` for unknown values
/// so callers can fall back gracefully instead of crashing on novel labels.
fn normalize_language(name: &str) -> Option<String> {
    let lower = name.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    if lower.len() == 2 && lower.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some(lower);
    }
    let code = match lower.as_str() {
        "english" => "en",
        "chinese" | "mandarin" => "zh",
        "japanese" => "ja",
        "korean" => "ko",
        "french" => "fr",
        "german" => "de",
        "spanish" => "es",
        "portuguese" => "pt",
        "russian" => "ru",
        "arabic" => "ar",
        "hindi" => "hi",
        "thai" => "th",
        "vietnamese" => "vi",
        "italian" => "it",
        "dutch" => "nl",
        "turkish" => "tr",
        "polish" => "pl",
        "ukrainian" => "uk",
        "indonesian" => "id",
        "malay" => "ms",
        _ => return None,
    };
    Some(code.to_string())
}

/// Wrap a rejected upload so [`crate::retry::is_retryable`] can still see the
/// status. A plain `bail!` erases it, which silently turns every transient 503
/// back into a lost dictation — hence the dedicated helper and its tests.
fn upload_error(provider_name: &str, status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    let sanitized = crate::retry::truncate_error_body(body);
    tracing::error!("{} HTTP {}: {}", provider_name, status, sanitized);
    crate::retry::HttpStatusError::new(
        status,
        format!("{provider_name} error ({status}): {sanitized}"),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_error_is_retryable_for_server_errors() {
        let err = upload_error("GLM-ASR", reqwest::StatusCode::SERVICE_UNAVAILABLE, "busy");
        assert!(
            crate::retry::is_retryable(&err),
            "a 503 upload must stay retryable, or a transient blip loses the dictation"
        );
        assert_eq!(
            err.to_string(),
            "GLM-ASR error (503 Service Unavailable): busy"
        );
    }

    #[test]
    fn upload_error_is_fatal_for_a_rejected_key() {
        let err = upload_error("OpenAI Whisper", reqwest::StatusCode::UNAUTHORIZED, "nope");
        assert!(
            !crate::retry::is_retryable(&err),
            "a rejected key must reach the user immediately"
        );
    }

    #[test]
    fn build_form_omits_language_when_set_is_empty() {
        let fields = build_form_text_fields("whisper-1", &[], &[]);
        assert!(
            !fields.iter().any(|(k, _)| k == "language"),
            "empty language set must omit the field so STT auto-detects"
        );
    }

    #[test]
    fn build_form_pins_single_language() {
        let fields = build_form_text_fields("whisper-1", &["en".to_string()], &[]);
        assert!(fields.iter().any(|(k, v)| k == "language" && v == "en"));
    }

    #[test]
    fn build_form_omits_language_when_multiple_selected() {
        let fields =
            build_form_text_fields("whisper-1", &["en".to_string(), "de".to_string()], &[]);
        assert!(
            !fields.iter().any(|(k, _)| k == "language"),
            "Whisper API cannot take multiple language hints; the request must omit the field and auto-detect"
        );
    }

    #[test]
    fn build_form_always_requests_verbose_json() {
        let fields = build_form_text_fields("whisper-1", &[], &[]);
        assert!(fields
            .iter()
            .any(|(k, v)| k == "response_format" && v == "verbose_json"));
    }

    #[test]
    fn build_form_includes_model_and_extras() {
        let fields = build_form_text_fields("glm-asr-2512", &[], &[("stream", "false")]);
        assert!(fields
            .iter()
            .any(|(k, v)| k == "model" && v == "glm-asr-2512"));
        assert!(fields.iter().any(|(k, v)| k == "stream" && v == "false"));
    }

    #[test]
    fn parse_response_extracts_text_when_no_language_field() {
        let body = r#"{"text": "Hello world"}"#;
        let (text, lang) = parse_response(body).unwrap();
        assert_eq!(text, "Hello world");
        assert_eq!(lang, None);
    }

    #[test]
    fn parse_response_extracts_text_and_detected_code_passthrough() {
        let body = r#"{"text": "Hallo Welt", "language": "de"}"#;
        let (text, lang) = parse_response(body).unwrap();
        assert_eq!(text, "Hallo Welt");
        assert_eq!(lang.as_deref(), Some("de"));
    }

    #[test]
    fn parse_response_normalizes_english_word_to_code() {
        let body = r#"{"text": "Hello", "language": "english"}"#;
        let (_, lang) = parse_response(body).unwrap();
        assert_eq!(lang.as_deref(), Some("en"));
    }

    #[test]
    fn parse_response_normalizes_german_word_to_code() {
        let body = r#"{"text": "Hallo", "language": "german"}"#;
        let (_, lang) = parse_response(body).unwrap();
        assert_eq!(lang.as_deref(), Some("de"));
    }

    #[test]
    fn parse_response_returns_none_for_unknown_language_name() {
        let body = r#"{"text": "Hi", "language": "klingon"}"#;
        let (_, lang) = parse_response(body).unwrap();
        assert_eq!(
            lang, None,
            "unknown language names fall back to None, not crash"
        );
    }

    #[test]
    fn parse_response_trims_text() {
        let body = r#"{"text": "  spaced  "}"#;
        let (text, _) = parse_response(body).unwrap();
        assert_eq!(text, "spaced");
    }

    #[test]
    fn normalize_language_passes_through_two_letter_iso_codes() {
        assert_eq!(normalize_language("en").as_deref(), Some("en"));
        assert_eq!(normalize_language("DE").as_deref(), Some("de"));
        assert_eq!(normalize_language(" fr ").as_deref(), Some("fr"));
    }

    #[test]
    fn normalize_language_rejects_empty_and_unknown() {
        assert_eq!(normalize_language(""), None);
        assert_eq!(normalize_language("   "), None);
        assert_eq!(normalize_language("klingon"), None);
    }
}

/// Configuration for a Whisper-compatible HTTP file-upload STT provider.
pub struct WhisperCompatConfig {
    pub provider_name: &'static str,
    pub endpoint: &'static str,
    pub model: &'static str,
    /// Extra form text fields (e.g. GLM-ASR needs "stream"="false").
    pub extra_fields: &'static [(&'static str, &'static str)],
}

/// Max audio buffer: ~24 MB PCM ≈ 12.5 min at 16kHz 16-bit mono.
/// Keeps the resulting WAV under 25 MB (OpenAI/Groq limit).
const MAX_AUDIO_BYTES: usize = 24 * 1024 * 1024;

/// Generic provider for any OpenAI Whisper-compatible transcription API.
/// Works with: OpenAI, Groq, SiliconFlow, GLM-ASR.
pub struct WhisperCompatProvider {
    provider_config: WhisperCompatConfig,
    stt_config: Option<SttConfig>,
    audio_buffer: Vec<u8>,
    client: reqwest::Client,
}

impl WhisperCompatProvider {
    /// Takes the app-wide pooled client (`crate::HttpClient`) rather than
    /// building one, so uploads reuse a warm connection — and so this provider
    /// cannot quietly opt out of the pool.
    pub fn new(provider_config: WhisperCompatConfig, client: reqwest::Client) -> Self {
        Self {
            provider_config,
            stt_config: None,
            audio_buffer: Vec::new(),
            client,
        }
    }

    /// Build a WAV file from raw PCM 16-bit mono audio. Public so test helpers can reuse it.
    pub fn build_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
        let data_len = pcm.len() as u32;
        let channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * (channels as u32) * (bits_per_sample as u32) / 8;
        let block_align = channels * bits_per_sample / 8;
        let file_size = 36 + data_len;

        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&file_size.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(pcm);
        wav
    }
}

#[async_trait]
impl SttProvider for WhisperCompatProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<()> {
        if config.api_key.is_empty() {
            anyhow::bail!("{} API key is empty", self.provider_config.provider_name);
        }
        self.stt_config = Some(config.clone());
        self.audio_buffer.clear();
        tracing::info!(
            "{} provider ready (buffering mode)",
            self.provider_config.provider_name
        );
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<()> {
        if self.audio_buffer.len() + chunk.len() > MAX_AUDIO_BYTES {
            anyhow::bail!(
                "{}: audio exceeds maximum length (~12 min)",
                self.provider_config.provider_name
            );
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
            tracing::info!(
                "{}: no audio buffered, skipping",
                self.provider_config.provider_name
            );
            return Ok(None);
        }

        let audio_len_secs = self.audio_buffer.len() as f64 / (config.sample_rate as f64 * 2.0);
        let wav_data = Self::build_wav(&self.audio_buffer, config.sample_rate);
        self.audio_buffer.clear();
        tracing::info!(
            "{}: sending {:.1}s of audio for transcription",
            self.provider_config.provider_name,
            audio_len_secs
        );

        let provider_name = self.provider_config.provider_name;

        // Safe to retry: one idempotent multipart POST, and nothing has been
        // shown to the user yet. A 429 or 502 here would otherwise throw away
        // the whole utterance the user just spoke.
        let (text, detected) =
            crate::retry::with_retry(&format!("{provider_name} transcription"), || async {
                // A multipart form is single-use, so each attempt rebuilds it —
                // including its copy of the WAV. That copy is the same order of
                // magnitude as the upload itself (~1 MB for a typical
                // utterance), so it is not worth avoiding.
                let file_part = reqwest::multipart::Part::bytes(wav_data.clone())
                    .file_name("audio.wav")
                    .mime_str("audio/wav")?;

                let mut form = reqwest::multipart::Form::new().part("file", file_part);
                for (k, v) in build_form_text_fields(
                    self.provider_config.model,
                    &config.languages,
                    self.provider_config.extra_fields,
                ) {
                    form = form.text(k, v);
                }

                let resp = self
                    .client
                    .post(self.provider_config.endpoint)
                    .header("Authorization", format!("Bearer {}", config.api_key))
                    .multipart(form)
                    .timeout(std::time::Duration::from_secs(60))
                    .send()
                    .await?;

                let status = resp.status();
                let body = resp.text().await?;

                if !status.is_success() {
                    return Err(upload_error(provider_name, status, &body));
                }

                parse_response(&body)
            })
            .await?;

        tracing::info!(
            "{} transcription: {} chars (detected={:?})",
            self.provider_config.provider_name,
            text.len(),
            detected
        );

        if text.is_empty() {
            Ok(None)
        } else {
            Ok(Some((text, detected)))
        }
    }

    fn name(&self) -> &str {
        self.provider_config.provider_name
    }
}

use anyhow::Result;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::{DisconnectResult, SttConfig, SttProvider, TranscriptEvent};

/// Pure URL builder. Deepgram's `language` parameter takes one ISO code or
/// the `multi` sentinel; we pin only when the user has selected exactly one
/// language, and otherwise use `multi` (which covers both empty selection and
/// "more than one expected language").
fn build_url(sample_rate: u32, smart_format: bool, languages: &[String]) -> String {
    let lang = if languages.len() == 1 {
        languages[0].as_str()
    } else {
        "multi"
    };
    format!(
        "wss://api.deepgram.com/v1/listen?\
         model=nova-3&\
         smart_format={}&\
         language={}&\
         punctuate=true&\
         utterances=true&\
         interim_results=true&\
         endpointing=150&\
         encoding=linear16&\
         sample_rate={}&\
         channels=1",
        smart_format, lang, sample_rate
    )
}

/// Parse one Deepgram `Results` message into a pipeline event. Pure so the
/// protocol handling is unit-testable; the WebSocket read stays in
/// `recv_transcript`.
fn parse_result_message(text: &str) -> Result<Option<TranscriptEvent>> {
    let v: serde_json::Value = serde_json::from_str(text)?;

    if v.get("type").and_then(|t| t.as_str()) == Some("Error") {
        let message = v["message"].as_str().unwrap_or("Unknown error").to_string();
        return Ok(Some(TranscriptEvent::Error { message }));
    }

    let alternative = &v["channel"]["alternatives"][0];
    let transcript = alternative["transcript"].as_str().unwrap_or("");
    if transcript.is_empty() {
        // Keep-alive, metadata and silent-segment messages all land here.
        return Ok(None);
    }

    if !v["is_final"].as_bool().unwrap_or(false) {
        return Ok(Some(TranscriptEvent::Partial {
            text: transcript.to_string(),
        }));
    }

    // A finalized segment always yields its text — including when
    // `speech_final` marks Deepgram's end-of-speech detection, because that
    // message carries the last words of the utterance. Returning `SpeechEnded`
    // instead would drop them, and nothing would notice: the pipeline ignores
    // `SpeechEnded` and drives finalization from the audio channel closing.
    let confidence = alternative["confidence"].as_f64().unwrap_or(0.0) as f32;
    // Deepgram reports the detected language on each result in multi mode (and
    // echoes the pinned code otherwise).
    let language = v["channel"]["detected_language"]
        .as_str()
        .map(|s| s.to_string());

    Ok(Some(TranscriptEvent::Final {
        text: transcript.to_string(),
        confidence,
        language,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn results_message(transcript: &str, is_final: bool, speech_final: bool) -> String {
        serde_json::json!({
            "is_final": is_final,
            "speech_final": speech_final,
            "channel": {
                "alternatives": [{ "transcript": transcript, "confidence": 0.98 }]
            }
        })
        .to_string()
    }

    #[test]
    fn speech_final_result_keeps_its_transcript() {
        // Regression: this branch used to return `SpeechEnded` and throw the
        // text away. Deepgram sets `speech_final` on the message carrying the
        // last words of an utterance, so for a short dictation that was the
        // whole transcript — silently lost.
        let parsed = parse_result_message(&results_message("hello world", true, true)).unwrap();
        match parsed {
            Some(TranscriptEvent::Final { text, .. }) => assert_eq!(text, "hello world"),
            other => panic!("expected Final with the transcript, got {other:?}"),
        }
    }

    #[test]
    fn finalized_segment_yields_final_with_confidence() {
        let parsed = parse_result_message(&results_message("finalized", true, false)).unwrap();
        match parsed {
            Some(TranscriptEvent::Final {
                text, confidence, ..
            }) => {
                assert_eq!(text, "finalized");
                assert!((confidence - 0.98).abs() < f32::EPSILON);
            }
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn interim_result_yields_partial() {
        let parsed = parse_result_message(&results_message("interim", false, false)).unwrap();
        match parsed {
            Some(TranscriptEvent::Partial { text }) => assert_eq!(text, "interim"),
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn empty_transcript_yields_nothing() {
        let parsed = parse_result_message(&results_message("", true, true)).unwrap();
        assert!(
            parsed.is_none(),
            "silent segments and keep-alives must not append empty text"
        );
    }

    #[test]
    fn metadata_message_yields_nothing() {
        let body = r#"{"type":"Metadata","duration":1.5}"#;
        assert!(parse_result_message(body).unwrap().is_none());
    }

    #[test]
    fn error_message_yields_error_event() {
        let body = r#"{"type":"Error","message":"invalid credentials"}"#;
        match parse_result_message(body).unwrap() {
            Some(TranscriptEvent::Error { message }) => assert_eq!(message, "invalid credentials"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn detected_language_is_threaded_through() {
        let body = serde_json::json!({
            "is_final": true,
            "channel": {
                "detected_language": "de",
                "alternatives": [{ "transcript": "hallo", "confidence": 0.9 }]
            }
        })
        .to_string();
        match parse_result_message(&body).unwrap() {
            Some(TranscriptEvent::Final { language, .. }) => {
                assert_eq!(language.as_deref(), Some("de"))
            }
            other => panic!("expected Final with a language, got {other:?}"),
        }
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse_result_message("not json").is_err());
    }

    #[test]
    fn build_url_uses_multi_when_languages_empty() {
        let url = build_url(16000, true, &[]);
        assert!(
            url.contains("language=multi"),
            "empty set should fall back to Deepgram's multi mode"
        );
    }

    #[test]
    fn build_url_pins_single_language() {
        let url = build_url(16000, true, &["de".to_string()]);
        assert!(url.contains("language=de"));
        assert!(!url.contains("language=multi"));
    }

    #[test]
    fn build_url_uses_multi_when_multiple_languages() {
        let url = build_url(
            16000,
            true,
            &["en".to_string(), "de".to_string(), "es".to_string()],
        );
        assert!(
            url.contains("language=multi"),
            "Deepgram takes one code or multi; multi covers the set"
        );
    }

    #[test]
    fn build_url_includes_sample_rate() {
        let url = build_url(48000, true, &[]);
        assert!(url.contains("sample_rate=48000"));
    }
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct DeepgramProvider {
    ws: Option<WsStream>,
}

impl Default for DeepgramProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepgramProvider {
    pub fn new() -> Self {
        Self { ws: None }
    }
}

#[async_trait]
impl SttProvider for DeepgramProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<()> {
        let url = build_url(config.sample_rate, config.smart_format, &config.languages);

        // Safe to retry: no session state exists yet, so a failed handshake is
        // a clean slate. Once connected, nothing else on this provider retries
        // — see `crate::retry`.
        let ws = crate::retry::with_retry("Deepgram connect", || async {
            // Each attempt builds its own request: the handshake carries a
            // single-use `Sec-WebSocket-Key`.
            let request = http::Request::builder()
                .uri(&url)
                .header("Authorization", format!("Token {}", config.api_key))
                .header("Host", "api.deepgram.com")
                .header("Connection", "Upgrade")
                .header("Upgrade", "websocket")
                .header("Sec-WebSocket-Version", "13")
                .header(
                    "Sec-WebSocket-Key",
                    tokio_tungstenite::tungstenite::handshake::client::generate_key(),
                )
                .body(())?;

            let (ws, _) = connect_async(request).await?;
            Ok(ws)
        })
        .await?;

        self.ws = Some(ws);
        tracing::info!("Deepgram WebSocket connected");
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<()> {
        if let Some(ws) = &mut self.ws {
            ws.send(Message::Binary(chunk.to_vec())).await?;
        }
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>> {
        let ws = match &mut self.ws {
            Some(ws) => ws,
            None => return Ok(None),
        };

        match ws.next().await {
            Some(Ok(Message::Text(text))) => parse_result_message(&text),
            Some(Ok(Message::Close(_))) => {
                tracing::info!("Deepgram WebSocket closed");
                Ok(None)
            }
            Some(Err(e)) => {
                tracing::error!("Deepgram WebSocket error: {}", e);
                Ok(Some(TranscriptEvent::Error {
                    message: e.to_string(),
                }))
            }
            _ => Ok(None),
        }
    }

    async fn disconnect(&mut self) -> Result<DisconnectResult> {
        if let Some(ws) = &mut self.ws {
            let close_msg = serde_json::json!({"type": "CloseStream"});
            let _ = ws.send(Message::Text(close_msg.to_string())).await;
            let _ = ws.close(None).await;
        }
        self.ws = None;
        tracing::info!("Deepgram disconnected");
        Ok(None)
    }

    fn name(&self) -> &str {
        "Deepgram Nova-3"
    }
}

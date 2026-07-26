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

#[cfg(test)]
mod tests {
    use super::*;

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
            Some(Ok(Message::Text(text))) => {
                let v: serde_json::Value = serde_json::from_str(&text)?;

                // Check for error
                if v.get("type").and_then(|t| t.as_str()) == Some("Error") {
                    let msg = v["message"].as_str().unwrap_or("Unknown error").to_string();
                    return Ok(Some(TranscriptEvent::Error { message: msg }));
                }

                // Parse transcript
                let transcript = v["channel"]["alternatives"][0]["transcript"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                if transcript.is_empty() {
                    return Ok(None);
                }

                let is_final = v["is_final"].as_bool().unwrap_or(false);
                let speech_final = v["speech_final"].as_bool().unwrap_or(false);

                if is_final {
                    let confidence = v["channel"]["alternatives"][0]["confidence"]
                        .as_f64()
                        .unwrap_or(0.0) as f32;

                    if speech_final {
                        return Ok(Some(TranscriptEvent::SpeechEnded));
                    }

                    // Deepgram returns the detected language on each result
                    // when in multi mode (and as a fixed echo when pinned).
                    let language = v["channel"]["detected_language"]
                        .as_str()
                        .map(|s| s.to_string());

                    Ok(Some(TranscriptEvent::Final {
                        text: transcript,
                        confidence,
                        language,
                    }))
                } else {
                    Ok(Some(TranscriptEvent::Partial { text: transcript }))
                }
            }
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

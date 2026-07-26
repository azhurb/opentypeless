use anyhow::Result;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::{DisconnectResult, SttConfig, SttProvider, TranscriptEvent};

use super::WsStream;

/// Parse one AssemblyAI streaming message into a pipeline event. Pure so the
/// protocol handling is unit-testable; the WebSocket read stays in
/// `recv_transcript`.
///
/// Only a **formatted** turn becomes `Final` — the unformatted ones are interim
/// text that AssemblyAI later replaces with a punctuated version, and treating
/// them as final would accumulate the same words twice.
fn parse_message(text: &str) -> Result<Option<TranscriptEvent>> {
    let v: serde_json::Value = serde_json::from_str(text)?;

    match v["type"].as_str().unwrap_or("") {
        "Begin" => {
            tracing::info!(
                "AssemblyAI session started: {}",
                v["id"].as_str().unwrap_or("")
            );
            Ok(None)
        }
        "Turn" => {
            let transcript = v["transcript"].as_str().unwrap_or("");
            if transcript.is_empty() {
                return Ok(None);
            }
            if v["turn_is_formatted"].as_bool().unwrap_or(false) {
                // AssemblyAI streaming doesn't currently report detected
                // language; the URL also doesn't accept a language hint. Both
                // are a follow-up.
                Ok(Some(TranscriptEvent::Final {
                    text: transcript.to_string(),
                    confidence: 1.0,
                    language: None,
                }))
            } else {
                Ok(Some(TranscriptEvent::Partial {
                    text: transcript.to_string(),
                }))
            }
        }
        "Termination" => {
            tracing::info!("AssemblyAI session terminated");
            Ok(Some(TranscriptEvent::SpeechEnded))
        }
        "Error" => Ok(Some(TranscriptEvent::Error {
            message: v["error"].as_str().unwrap_or("Unknown error").to_string(),
        })),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn_message(transcript: &str, formatted: bool) -> String {
        serde_json::json!({
            "type": "Turn",
            "transcript": transcript,
            "turn_is_formatted": formatted,
        })
        .to_string()
    }

    #[test]
    fn formatted_turn_is_final() {
        match parse_message(&turn_message("Hello, world.", true)).unwrap() {
            Some(TranscriptEvent::Final { text, .. }) => assert_eq!(text, "Hello, world."),
            other => panic!("expected Final, got {other:?}"),
        }
    }

    #[test]
    fn unformatted_turn_is_only_partial() {
        // Accumulating these would duplicate the words that the formatted turn
        // repeats a moment later.
        match parse_message(&turn_message("hello world", false)).unwrap() {
            Some(TranscriptEvent::Partial { text }) => assert_eq!(text, "hello world"),
            other => panic!("expected Partial, got {other:?}"),
        }
    }

    #[test]
    fn empty_turn_yields_nothing() {
        assert!(parse_message(&turn_message("", true)).unwrap().is_none());
    }

    #[test]
    fn termination_signals_speech_ended() {
        // This is what stops the post-close drain.
        let body = r#"{"type":"Termination"}"#;
        assert!(matches!(
            parse_message(body).unwrap(),
            Some(TranscriptEvent::SpeechEnded)
        ));
    }

    #[test]
    fn error_message_yields_error_event() {
        let body = r#"{"type":"Error","error":"bad key"}"#;
        match parse_message(body).unwrap() {
            Some(TranscriptEvent::Error { message }) => assert_eq!(message, "bad key"),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn begin_and_unknown_types_yield_nothing() {
        assert!(parse_message(r#"{"type":"Begin","id":"abc"}"#)
            .unwrap()
            .is_none());
        assert!(parse_message(r#"{"type":"SomethingNew"}"#)
            .unwrap()
            .is_none());
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse_message("not json").is_err());
    }
}

pub struct AssemblyAiProvider {
    ws: Option<WsStream>,
}

impl Default for AssemblyAiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AssemblyAiProvider {
    pub fn new() -> Self {
        Self { ws: None }
    }

    fn build_url(config: &SttConfig) -> String {
        format!(
            "wss://streaming.assemblyai.com/v3/ws?\
             sample_rate={}&\
             format_turns=true",
            config.sample_rate
        )
    }
}

#[async_trait]
impl SttProvider for AssemblyAiProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<()> {
        let url = Self::build_url(config);

        // Safe to retry: no session state exists yet, so a failed handshake is
        // a clean slate. Once connected, nothing else on this provider retries
        // — see `crate::retry`.
        let ws = crate::retry::with_retry("AssemblyAI connect", || async {
            // Each attempt builds its own request: the handshake carries a
            // single-use `Sec-WebSocket-Key`.
            let request = http::Request::builder()
                .uri(&url)
                .header("Authorization", &config.api_key)
                .header("Host", "streaming.assemblyai.com")
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
        tracing::info!("AssemblyAI WebSocket connected");
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
            Some(Ok(Message::Text(text))) => parse_message(&text),
            Some(Ok(Message::Close(_))) => {
                tracing::info!("AssemblyAI WebSocket closed");
                Ok(None)
            }
            Some(Err(e)) => {
                tracing::error!("AssemblyAI WebSocket error: {}", e);
                Ok(Some(TranscriptEvent::Error {
                    message: e.to_string(),
                }))
            }
            _ => Ok(None),
        }
    }

    async fn disconnect(&mut self) -> Result<DisconnectResult> {
        let drained = match &mut self.ws {
            Some(ws) => {
                // `Terminate` is what makes AssemblyAI emit the formatted
                // version of the turn in progress — the only kind that becomes a
                // `Final` — so read until it says `Termination` before closing.
                let terminate = serde_json::json!({"type": "Terminate"});
                let _ = ws.send(Message::Text(terminate.to_string())).await;
                let drained = super::drain_final_text(ws, parse_message).await;
                let _ = ws.close(None).await;
                drained
            }
            None => None,
        };
        self.ws = None;
        if let Some((text, _)) = &drained {
            tracing::info!("AssemblyAI flushed {} chars on close", text.len());
        }
        tracing::info!("AssemblyAI disconnected");
        Ok(drained)
    }

    fn name(&self) -> &str {
        "AssemblyAI"
    }
}

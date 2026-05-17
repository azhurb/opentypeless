use anyhow::Result;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::{DisconnectResult, SttConfig, SttProvider, TranscriptEvent};

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

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
            Some(Ok(Message::Text(text))) => {
                let v: serde_json::Value = serde_json::from_str(&text)?;
                let msg_type = v["type"].as_str().unwrap_or("");

                match msg_type {
                    "Begin" => {
                        tracing::info!(
                            "AssemblyAI session started: {}",
                            v["id"].as_str().unwrap_or("")
                        );
                        Ok(None)
                    }
                    "Turn" => {
                        let transcript = v["transcript"].as_str().unwrap_or("").to_string();
                        if transcript.is_empty() {
                            return Ok(None);
                        }
                        let is_formatted = v["turn_is_formatted"].as_bool().unwrap_or(false);
                        if is_formatted {
                            // AssemblyAI streaming doesn't currently report
                            // detected language; the URL also doesn't accept
                            // a language hint. Both are a follow-up.
                            Ok(Some(TranscriptEvent::Final {
                                text: transcript,
                                confidence: 1.0,
                                language: None,
                            }))
                        } else {
                            Ok(Some(TranscriptEvent::Partial { text: transcript }))
                        }
                    }
                    "Termination" => {
                        tracing::info!("AssemblyAI session terminated");
                        Ok(Some(TranscriptEvent::SpeechEnded))
                    }
                    "Error" => {
                        let msg = v["error"].as_str().unwrap_or("Unknown error").to_string();
                        Ok(Some(TranscriptEvent::Error { message: msg }))
                    }
                    _ => Ok(None),
                }
            }
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
        if let Some(ws) = &mut self.ws {
            let terminate = serde_json::json!({"type": "Terminate"});
            let _ = ws.send(Message::Text(terminate.to_string())).await;
            let _ = ws.close(None).await;
        }
        self.ws = None;
        tracing::info!("AssemblyAI disconnected");
        Ok(None)
    }

    fn name(&self) -> &str {
        "AssemblyAI"
    }
}

pub mod assemblyai;
pub mod deepgram;
pub mod gemini;
pub mod whisper_compat;

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;

use whisper_compat::{WhisperCompatConfig, WhisperCompatProvider};

/// The WebSocket stream both streaming providers hold.
pub(crate) type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// How long to wait for the next message while draining. An idle gap this long
/// ends the drain.
const DRAIN_IDLE_MS: u64 = 150;

/// Approximate ceiling on the whole drain, however much the provider sends.
/// Checked between reads, so the real bound is this plus one idle window.
const DRAIN_TOTAL_MS: u64 = 600;

/// Read whatever a streaming provider flushes after being told to finish.
///
/// Both streaming providers used to send their close signal and shut the socket
/// in the same breath, so anything the server sent *in response* was dropped —
/// for Deepgram the results still pending at `CloseStream`, for AssemblyAI the
/// formatted version of the final turn (the only kind that becomes a `Final`).
/// Since the pipeline accumulates text from `Final` events only, that silently
/// cost the tail of an utterance, and all of it for a dictation short enough to
/// be one turn.
///
/// Bounded twice on purpose: this sits between the user releasing the hotkey and
/// text appearing, so a fixed wait would be a latency regression on every
/// dictation. A provider that answers promptly — the expected case — costs only
/// its own flush time, and one that says nothing costs `DRAIN_IDLE_MS`.
///
/// `TranscriptEvent::SpeechEnded` is the stop signal: AssemblyAI's `Termination`
/// maps to it, which is the server saying it has nothing left to send.
pub(crate) async fn drain_final_text<P>(ws: &mut WsStream, parse: P) -> DisconnectResult
where
    P: Fn(&str) -> Result<Option<TranscriptEvent>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_millis(DRAIN_TOTAL_MS);
    let idle = Duration::from_millis(DRAIN_IDLE_MS);
    let mut text = String::new();
    let mut language = None;

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(idle, ws.next()).await {
            // Idle gap: the provider has stopped talking.
            Err(_) => break,
            // Stream ended, socket closed, or errored — nothing more is coming.
            Ok(None) | Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) => break,
            Ok(Some(Ok(Message::Text(raw)))) => match parse(&raw) {
                Ok(Some(TranscriptEvent::Final {
                    text: chunk,
                    language: detected,
                    ..
                })) => {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&chunk);
                    if detected.is_some() {
                        language = detected;
                    }
                }
                Ok(Some(TranscriptEvent::SpeechEnded)) => break,
                // Partials are superseded by the final that follows, and a parse
                // error is moot with the session already ending.
                _ => {}
            },
            // Metadata, pings, binary frames.
            Ok(Some(Ok(_))) => {}
        }
    }

    if text.is_empty() {
        None
    } else {
        Some((text, language))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    pub api_key: String,
    /// User-selected language hints. Empty = let the provider auto-detect.
    /// Whisper-compatible providers accept at most one hint at the wire,
    /// so adapters pin only when `languages.len() == 1` and otherwise omit
    /// the field (auto-detect). Deepgram's `multi` mode covers the >1 case
    /// natively.
    pub languages: Vec<String>,
    pub smart_format: bool,
    pub sample_rate: u32,
    /// Terms from the user's dictionary, for providers that accept vocabulary
    /// biasing. Only `gemini-transcribe` reads it today; the others ignore it,
    /// the same way the Whisper-compatible ones ignore `smart_format`.
    pub custom_vocabulary: Vec<String>,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            languages: Vec::new(),
            smart_format: true,
            sample_rate: 16000,
            custom_vocabulary: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TranscriptEvent {
    Partial {
        text: String,
    },
    Final {
        text: String,
        confidence: f32,
        /// ISO-639-1 code of the language the provider detected, when reported.
        language: Option<String>,
    },
    SpeechStarted,
    SpeechEnded,
    Error {
        message: String,
    },
}

/// Result returned by `SttProvider::disconnect` for file-based providers that
/// produce the transcript on close. Streaming providers return `None` here and
/// emit `TranscriptEvent::Final` instead.
pub type DisconnectResult = Option<(String, Option<String>)>;

#[async_trait]
pub trait SttProvider: Send + Sync {
    async fn connect(&mut self, config: &SttConfig) -> Result<()>;
    async fn send_audio(&mut self, chunk: &[u8]) -> Result<()>;
    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>>;
    /// Disconnect and optionally return a final `(text, detected_language)` pair
    /// for file-based providers. Streaming providers return `Ok(None)`.
    async fn disconnect(&mut self) -> Result<DisconnectResult>;
    fn name(&self) -> &str;
}

#[cfg(test)]
mod factory_tests {
    use super::*;

    /// Every provider ID the Settings dropdown offers, paired with the provider
    /// `create_provider` must hand back for it.
    ///
    /// This list is the frontend's `STT_PROVIDERS` in `src/lib/constants.ts`.
    /// The two must agree, and nothing in the build enforces that, which is not
    /// a hypothetical: `deepgram` sat in the dropdown from the initial commit
    /// with no arm in this factory, so choosing it silently fell through to the
    /// GLM-ASR default and authenticated a GLM-ASR request with a Deepgram key.
    /// That went unnoticed for months because the symptom is a confusing auth
    /// error rather than a crash. A missing arm now fails here instead.
    const DROPDOWN_IDS: &[(&str, &str)] = &[
        ("deepgram", "Deepgram Nova-3"),
        ("assemblyai", "AssemblyAI"),
        ("gemini-transcribe", "Gemini Transcribe"),
        ("glm-asr", "GLM-ASR"),
        ("openai-whisper", "OpenAI Whisper"),
        ("groq-whisper", "Groq Whisper"),
        ("siliconflow", "SiliconFlow"),
    ];

    #[test]
    fn every_dropdown_id_builds_its_own_provider() {
        for (id, expected) in DROPDOWN_IDS {
            let provider = create_provider(id, reqwest::Client::new());
            assert_eq!(
                provider.name(),
                *expected,
                "STT id `{id}` built `{}` instead of `{expected}`. If this id was just added to \
                 src/lib/constants.ts, it also needs an arm in create_provider — without one it \
                 falls through to the GLM-ASR default and sends this provider's key to Zhipu.",
                provider.name()
            );
        }
    }

    #[test]
    fn unknown_id_falls_back_to_the_documented_default() {
        // The fallback is deliberate, so it is pinned rather than left implicit:
        // a stored config from a future version, or a hand-edited settings file,
        // has to land somewhere predictable.
        let provider = create_provider("not-a-provider", reqwest::Client::new());
        assert_eq!(provider.name(), "GLM-ASR");
    }
}

pub fn create_provider(provider_name: &str, client: reqwest::Client) -> Box<dyn SttProvider> {
    let make = |cfg: WhisperCompatConfig| -> Box<dyn SttProvider> {
        Box::new(WhisperCompatProvider::new(cfg, client.clone()))
    };
    match provider_name {
        "deepgram" => Box::new(deepgram::DeepgramProvider::new()),
        "assemblyai" => Box::new(assemblyai::AssemblyAiProvider::new()),
        "gemini-transcribe" => Box::new(gemini::GeminiTranscribeProvider::new(client.clone())),
        "glm-asr" => make(WhisperCompatConfig {
            provider_name: "GLM-ASR",
            endpoint: "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
            model: "glm-asr-2512",
            extra_fields: &[("stream", "false")],
        }),
        "openai-whisper" => make(WhisperCompatConfig {
            provider_name: "OpenAI Whisper",
            endpoint: "https://api.openai.com/v1/audio/transcriptions",
            model: "whisper-1",
            extra_fields: &[],
        }),
        "groq-whisper" => make(WhisperCompatConfig {
            provider_name: "Groq Whisper",
            endpoint: "https://api.groq.com/openai/v1/audio/transcriptions",
            model: "whisper-large-v3-turbo",
            extra_fields: &[],
        }),
        "siliconflow" => make(WhisperCompatConfig {
            provider_name: "SiliconFlow",
            endpoint: "https://api.siliconflow.cn/v1/audio/transcriptions",
            model: "FunAudioLLM/SenseVoiceSmall",
            extra_fields: &[],
        }),
        _ => make(WhisperCompatConfig {
            provider_name: "GLM-ASR",
            endpoint: "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
            model: "glm-asr-2512",
            extra_fields: &[("stream", "false")],
        }),
    }
}

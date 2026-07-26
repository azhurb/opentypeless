pub mod assemblyai;
pub mod deepgram;
pub mod whisper_compat;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use whisper_compat::{WhisperCompatConfig, WhisperCompatProvider};

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
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            languages: Vec::new(),
            smart_format: true,
            sample_rate: 16000,
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

pub fn create_provider(provider_name: &str, client: reqwest::Client) -> Box<dyn SttProvider> {
    let make = |cfg: WhisperCompatConfig| -> Box<dyn SttProvider> {
        Box::new(WhisperCompatProvider::new(cfg, client.clone()))
    };
    match provider_name {
        "assemblyai" => Box::new(assemblyai::AssemblyAiProvider::new()),
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

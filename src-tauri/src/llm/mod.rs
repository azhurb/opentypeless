pub mod openai;
pub mod prompt;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub max_tokens: u32,
    pub temperature: f64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: "glm-4.7".to_string(),
            base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            max_tokens: 4096,
            temperature: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolishRequest {
    pub raw_text: String,
    pub app_type: AppType,
    pub dictionary: Vec<String>,
    pub translate_enabled: bool,
    pub target_lang: String,
    pub selected_text: Option<String>,
    /// ISO-639-1 code reported by the STT for this utterance, when available.
    pub detected_language: Option<String>,
    /// Codes the user has marked as expected. Empty = auto-detect.
    pub user_languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolishResponse {
    pub polished_text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum AppType {
    Email,
    Chat,
    Code,
    Document,
    #[default]
    General,
}

/// Callback for streaming LLM chunks to the frontend
pub type ChunkCallback = Box<dyn Fn(&str) + Send + Sync>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn polish(
        &self,
        config: &LlmConfig,
        req: &PolishRequest,
        on_chunk: Option<&ChunkCallback>,
    ) -> Result<PolishResponse>;

    fn name(&self) -> &str;
}

pub fn create_provider(_provider_name: &str, client: reqwest::Client) -> Box<dyn LlmProvider> {
    Box::new(openai::OpenAiProvider::new(client))
}

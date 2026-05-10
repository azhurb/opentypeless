// App metadata
export const APP_NAME = 'OpenTypeless'
export const APP_VERSION = 'v0.1.0'
export const APP_REPO_URL = 'https://github.com/azhurb/opentypeless'
export const APP_LICENSE_URL = 'https://github.com/azhurb/opentypeless/blob/main/LICENSE'

export const STT_PROVIDERS = [
  { value: 'deepgram', label: 'Deepgram Nova-3' },
  { value: 'assemblyai', label: 'AssemblyAI' },
  { value: 'glm-asr', label: 'GLM-ASR (智谱)' },
  { value: 'openai-whisper', label: 'OpenAI Whisper' },
  { value: 'groq-whisper', label: 'Groq Whisper' },
  { value: 'siliconflow', label: 'SiliconFlow (硅基流动)' },
] as const

export const LLM_PROVIDERS = [
  { value: 'zhipu', label: '智谱 (Zhipu)' },
  { value: 'deepseek', label: 'DeepSeek' },
  { value: 'siliconflow', label: '硅基流动 (SiliconFlow)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'gemini', label: 'Google Gemini' },
  { value: 'moonshot', label: 'Moonshot (Kimi)' },
  { value: 'qwen', label: '通义千问 (Qwen)' },
  { value: 'groq', label: 'Groq' },
  { value: 'claude', label: 'Claude' },
  { value: 'ollama', label: 'Ollama (Local)' },
  { value: 'openrouter', label: 'OpenRouter' },
] as const

export const LLM_DEFAULT_CONFIG: Record<string, { baseUrl: string; model: string }> = {
  zhipu: { baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-flash' },
  deepseek: { baseUrl: 'https://api.deepseek.com/v1', model: 'deepseek-chat' },
  siliconflow: { baseUrl: 'https://api.siliconflow.cn/v1', model: 'Qwen/Qwen2.5-7B-Instruct' },
  openai: { baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o-mini' },
  gemini: {
    baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
    model: 'gemini-2.0-flash',
  },
  moonshot: { baseUrl: 'https://api.moonshot.cn/v1', model: 'moonshot-v1-8k' },
  qwen: { baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', model: 'qwen-turbo' },
  groq: { baseUrl: 'https://api.groq.com/openai/v1', model: 'llama-3.3-70b-versatile' },
  claude: { baseUrl: 'https://openrouter.ai/api/v1', model: 'anthropic/claude-sonnet-4' },
  ollama: { baseUrl: 'http://localhost:11434/v1', model: 'llama3.2' },
  openrouter: { baseUrl: 'https://openrouter.ai/api/v1', model: 'openai/gpt-4o-mini' },
}

export const LANGUAGES = [
  { value: 'multi', label: 'Auto Detect' },
  { value: 'zh', label: '中文 (Chinese)' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語 (Japanese)' },
  { value: 'ko', label: '한국어 (Korean)' },
  { value: 'fr', label: 'Français (French)' },
  { value: 'de', label: 'Deutsch (German)' },
  { value: 'es', label: 'Español (Spanish)' },
  { value: 'pt', label: 'Português (Portuguese)' },
  { value: 'ru', label: 'Русский (Russian)' },
  { value: 'ar', label: 'العربية (Arabic)' },
  { value: 'hi', label: 'हिन्दी (Hindi)' },
  { value: 'th', label: 'ไทย (Thai)' },
  { value: 'vi', label: 'Tiếng Việt (Vietnamese)' },
  { value: 'it', label: 'Italiano (Italian)' },
  { value: 'nl', label: 'Nederlands (Dutch)' },
  { value: 'tr', label: 'Türkçe (Turkish)' },
  { value: 'pl', label: 'Polski (Polish)' },
  { value: 'uk', label: 'Українська (Ukrainian)' },
  { value: 'id', label: 'Bahasa Indonesia' },
  { value: 'ms', label: 'Bahasa Melayu (Malay)' },
] as const

export const TARGET_LANGUAGES = [
  { value: 'en', label: 'English' },
  { value: 'zh', label: '中文 (Chinese)' },
  { value: 'ja', label: '日本語 (Japanese)' },
  { value: 'ko', label: '한국어 (Korean)' },
  { value: 'fr', label: 'Français (French)' },
  { value: 'de', label: 'Deutsch (German)' },
  { value: 'es', label: 'Español (Spanish)' },
  { value: 'pt', label: 'Português (Portuguese)' },
  { value: 'ru', label: 'Русский (Russian)' },
  { value: 'ar', label: 'العربية (Arabic)' },
  { value: 'hi', label: 'हिन्दी (Hindi)' },
  { value: 'th', label: 'ไทย (Thai)' },
  { value: 'vi', label: 'Tiếng Việt (Vietnamese)' },
  { value: 'it', label: 'Italiano (Italian)' },
  { value: 'nl', label: 'Nederlands (Dutch)' },
  { value: 'tr', label: 'Türkçe (Turkish)' },
  { value: 'pl', label: 'Polski (Polish)' },
  { value: 'uk', label: 'Українська (Ukrainian)' },
  { value: 'id', label: 'Bahasa Indonesia' },
  { value: 'ms', label: 'Bahasa Melayu (Malay)' },
] as const

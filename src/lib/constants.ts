// App metadata
export const APP_NAME = 'OpenTypeless'
// No APP_VERSION here on purpose: the release workflow rewrites the version in
// package.json / tauri.conf.json / Cargo.toml at build time and never commits
// it, so any constant in this file is permanently stale. AboutPane reads the
// real version from the bundle via `getVersion()`.
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

/**
 * Default endpoint and model per provider. The model field is free text in
 * Settings, so these only seed a fresh pick — an existing user's stored model is
 * never rewritten, which is why a dead default strands new users specifically.
 *
 * Picking a default, in priority order:
 *  1. Currently served, with no announced shutdown inside ~6 months.
 *  2. Cheap and fast. Polishing one dictated sentence is a formatting job; the
 *     wait is in front of a user who is watching for their text to appear.
 *  3. Reasoning off, ideally not implemented at all. A thinking model spends
 *     latency and tokens deliberating over comma placement, and emits a
 *     `<think>` scratchpad that `llm::think` then has to strip back out. Where
 *     the best available model does reason, the switch that turns it off belongs
 *     in `llm::openai::reasoning_params`, not in a comment here.
 *
 * Verified against provider docs on 2026-08-15. Base URLs carry no trailing
 * slash: the request path is built as `{baseUrl}/chat/completions`.
 */
export const LLM_DEFAULT_CONFIG: Record<string, { baseUrl: string; model: string }> = {
  // Bare `glm-4-flash` is no longer an enumerated ID; the dated build is, and it
  // is free and predates GLM's thinking mode entirely.
  zhipu: { baseUrl: 'https://open.bigmodel.cn/api/paas/v4', model: 'glm-4-flash-250414' },
  // `deepseek-chat` was discontinued on 2026-07-24 along with `deepseek-reasoner`.
  deepseek: { baseUrl: 'https://api.deepseek.com/v1', model: 'deepseek-v4-flash' },
  siliconflow: { baseUrl: 'https://api.siliconflow.cn/v1', model: 'Qwen/Qwen2.5-7B-Instruct' },
  // gpt-4o-mini is off the promoted list but is served, not deprecated, and
  // genuinely has no reasoning mode — which beats a newer tier that reasons by
  // default and rejects the `temperature` this app sends.
  openai: { baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o-mini' },
  // Gemini 2.0 Flash was shut down on 2026-06-01. 2.5-flash-lite is the cheapest
  // model whose thinking is off by default; every 3.x flash tier reasons and
  // Google documents that it cannot be fully disabled.
  gemini: {
    baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
    model: 'gemini-2.5-flash-lite',
  },
  // The moonshot-v1 series goes offline platform-wide on 2026-08-31.
  moonshot: { baseUrl: 'https://api.moonshot.cn/v1', model: 'kimi-k2.5' },
  // Alibaba names qwen-flash as qwen-turbo's replacement; turbo is frozen and on
  // a retirement list. Thinking is off by default on qwen-flash.
  qwen: { baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1', model: 'qwen-flash' },
  // llama-3.3-70b-versatile is decommissioned on 2026-08-16. gpt-oss-120b is
  // production rather than preview, and reports its reasoning in a separate
  // response field, so `content` arrives clean.
  groq: { baseUrl: 'https://api.groq.com/openai/v1', model: 'openai/gpt-oss-120b' },
  // claude-sonnet-4 was retired from Anthropic's own API on 2026-06-15 and now
  // routes only via resellers. Haiku 4.5 is the current small Claude and the
  // last one without adaptive thinking on by default.
  claude: { baseUrl: 'https://openrouter.ai/api/v1', model: 'anthropic/claude-haiku-4.5' },
  ollama: { baseUrl: 'http://localhost:11434/v1', model: 'llama3.2' },
  openrouter: { baseUrl: 'https://openrouter.ai/api/v1', model: 'openai/gpt-4o-mini' },
}

export const LANGUAGES = [
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

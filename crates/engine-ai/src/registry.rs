//! Model registry: provider definitions, model metadata, and runtime detection.
//!
//! The registry holds both **builtin** models (hardcoded known-good defaults)
//! and **detected** models (discovered at runtime via provider APIs).

use engine_core::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

// ─── Provider Kind ────────────────────────────────────────────────────────────

/// Supported AI provider backends.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Anthropic Messages API.
    Anthropic,
    /// OpenAI Chat Completions API.
    OpenAI,
    /// OpenAI Codex via ChatGPT OAuth.
    CodexOAuth,
    /// Google Gemini (AI Studio / Vertex).
    Gemini,
    /// Local Ollama instance.
    Ollama,
    /// User-defined OpenAI-compatible endpoint.
    Custom,
    /// Xiaomi MiMo (unified, with region/billing config).
    Mimo,
    /// DeepSeek API.
    DeepSeek,
    /// GLM/Zhipu AI (unified, with region/billing config).
    Glm,
}

impl ProviderKind {
    /// Returns all non-custom provider kinds in display order.
    pub fn builtin_providers() -> &'static [ProviderKind] {
        &[
            ProviderKind::Anthropic,
            ProviderKind::OpenAI,
            ProviderKind::CodexOAuth,
            ProviderKind::Gemini,
            ProviderKind::Ollama,
            ProviderKind::Mimo,
            ProviderKind::DeepSeek,
            ProviderKind::Glm,
        ]
    }

    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::OpenAI => "OpenAI",
            Self::CodexOAuth => "Codex OAuth (ChatGPT)",
            Self::Gemini => "Google Gemini",
            Self::Ollama => "Ollama (Local)",
            Self::Custom => "Custom (OpenAI-Compatible)",
            Self::Mimo => "Xiaomi MiMo",
            Self::DeepSeek => "DeepSeek",
            Self::Glm => "GLM/Zhipu AI",
        }
    }

    /// Whether this provider requires an API key.
    pub fn requires_api_key(&self) -> bool {
        matches!(
            self,
            Self::Anthropic
                | Self::OpenAI
                | Self::Gemini
                | Self::Custom
                | Self::Mimo
                | Self::DeepSeek
                | Self::Glm
        )
    }

    /// Whether the user must provide a custom endpoint URL.
    /// MiMo and GLM endpoints are determined by config (region/billing), not user input.
    pub fn requires_endpoint(&self) -> bool {
        matches!(self, Self::Custom)
    }

    /// Whether the user may override this provider's endpoint URL.
    pub fn endpoint_configurable(&self) -> bool {
        matches!(self, Self::Ollama | Self::Custom)
    }

    /// Whether the endpoint is auto-determined by provider config.
    pub fn endpoint_auto_determined(&self) -> bool {
        matches!(self, Self::Mimo | Self::Glm)
    }

    /// Default endpoint for this provider (if any).
    /// For MiMo and GLM, this returns None since endpoints are determined by config.
    pub fn default_endpoint(&self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("https://api.anthropic.com"),
            Self::OpenAI => Some("https://api.openai.com/v1"),
            Self::CodexOAuth => Some("https://chatgpt.com/backend-api/codex/responses"),
            Self::Gemini => Some("https://generativelanguage.googleapis.com"),
            Self::Ollama => Some("http://localhost:11434"),
            Self::Custom => None,
            Self::Mimo => None, // Determined by MimoConfig
            Self::DeepSeek => Some("https://api.deepseek.com"),
            Self::Glm => None, // Determined by GlmConfig
        }
    }
}

/// MiMo endpoint configuration helper.
pub struct MimoEndpoints;

impl MimoEndpoints {
    /// Returns the base URL for MiMo based on billing mode and region.
    pub fn base_url(
        billing: &engine_editor::BillingMode,
        region: &engine_editor::MimoRegion,
    ) -> &'static str {
        use engine_editor::{BillingMode, MimoRegion};
        match (billing, region) {
            (BillingMode::Subscription, MimoRegion::China) => {
                "https://token-plan-cn.xiaomimimo.com/v1"
            }
            (BillingMode::Subscription, MimoRegion::Singapore) => {
                "https://token-plan-sgp.xiaomimimo.com/v1"
            }
            (BillingMode::Subscription, MimoRegion::Europe) => {
                "https://token-plan-ams.xiaomimimo.com/v1"
            }
            (BillingMode::Api, _) => "https://api.xiaomimimo.com/v1",
        }
    }
}

/// GLM endpoint configuration helper.
pub struct GlmEndpoints;

impl GlmEndpoints {
    /// Returns the base URL for GLM based on billing mode and region.
    pub fn base_url(
        billing: &engine_editor::BillingMode,
        region: &engine_editor::GlmRegion,
    ) -> &'static str {
        use engine_editor::{BillingMode, GlmRegion};
        match (billing, region) {
            (BillingMode::Subscription, GlmRegion::Bigmodel) => {
                "https://open.bigmodel.cn/api/coding/paas/v4"
            }
            (BillingMode::Subscription, GlmRegion::Zai) => "https://api.z.ai/api/coding/paas/v4",
            (BillingMode::Api, GlmRegion::Bigmodel) => "https://open.bigmodel.cn/api/paas/v4",
            (BillingMode::Api, GlmRegion::Zai) => "https://api.z.ai/api/paas/v4",
        }
    }
}

// ─── Model Capabilities ───────────────────────────────────────────────────────

/// Capability flags for a model.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Whether the model supports extended thinking / chain-of-thought.
    pub can_reason: bool,
    /// Whether the model accepts image inputs.
    pub supports_vision: bool,
    /// Whether the model supports tool/function calling.
    pub supports_tools: bool,
}

// ─── Model Info ───────────────────────────────────────────────────────────────

/// Metadata for a single model variant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelInfo {
    /// The model ID string sent to the API (e.g. "claude-sonnet-4-6").
    pub id: String,
    /// Human-readable display name (e.g. "Claude Sonnet 4").
    pub display_name: String,
    /// Which provider this model belongs to.
    pub provider: ProviderKind,
    /// Context window size in tokens.
    pub context_window: u32,
    /// Recommended max output tokens.
    pub default_max_tokens: u32,
    /// Model capabilities.
    pub capabilities: ModelCapabilities,
}

// ─── Model Registry ───────────────────────────────────────────────────────────

/// Holds known and detected models.
pub struct ModelRegistry {
    builtin: Vec<ModelInfo>,
    detected: Vec<ModelInfo>,
}

impl ModelRegistry {
    /// Creates a registry populated with all builtin models.
    pub fn new() -> Self {
        Self {
            builtin: builtin_models(),
            detected: Vec::new(),
        }
    }

    /// Returns builtin models for a specific provider.
    pub fn builtin_for(&self, provider: &ProviderKind) -> Vec<&ModelInfo> {
        self.builtin
            .iter()
            .filter(|m| &m.provider == provider)
            .collect()
    }

    /// Returns detected models for a specific provider.
    pub fn detected_for(&self, provider: &ProviderKind) -> Vec<&ModelInfo> {
        self.detected
            .iter()
            .filter(|m| &m.provider == provider)
            .collect()
    }

    /// Returns all models (builtin + detected) for a provider.
    pub fn models_for(&self, provider: &ProviderKind) -> Vec<&ModelInfo> {
        self.builtin
            .iter()
            .chain(self.detected.iter())
            .filter(|m| &m.provider == provider)
            .collect()
    }

    /// Replaces detected models for a provider with a new list.
    pub fn set_detected(&mut self, provider: &ProviderKind, models: Vec<ModelInfo>) {
        self.detected.retain(|m| &m.provider != provider);
        self.detected.extend(models);
    }

    /// Looks up a model by ID across all providers.
    pub fn find_by_id(&self, id: &str) -> Option<&ModelInfo> {
        self.builtin
            .iter()
            .chain(self.detected.iter())
            .find(|m| m.id == id)
    }

    /// Returns the recommended default model for a provider.
    pub fn default_model_for(&self, provider: &ProviderKind) -> Option<&ModelInfo> {
        self.builtin_for(provider).into_iter().next()
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Builtin Models ───────────────────────────────────────────────────────────

fn builtin_models() -> Vec<ModelInfo> {
    vec![
        // ── Anthropic ──
        ModelInfo {
            id: "claude-sonnet-4-20250514".into(),
            display_name: "Claude Sonnet 4".into(),
            provider: ProviderKind::Anthropic,
            context_window: 200_000,
            default_max_tokens: 16_384,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gpt-5.4".into(),
            display_name: "GPT-5.4 Codex (OAuth)".into(),
            provider: ProviderKind::CodexOAuth,
            context_window: 400_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gpt-5.5".into(),
            display_name: "GPT-5.5 Codex (OAuth)".into(),
            provider: ProviderKind::CodexOAuth,
            context_window: 400_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gpt-5.3-codex".into(),
            display_name: "GPT-5.3 Codex (OAuth)".into(),
            provider: ProviderKind::CodexOAuth,
            context_window: 400_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gpt-5.3-codex-spark".into(),
            display_name: "GPT-5.3 Codex Spark (OAuth)".into(),
            provider: ProviderKind::CodexOAuth,
            context_window: 400_000,
            default_max_tokens: 64_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gpt-5.2".into(),
            display_name: "GPT-5.2 Codex (OAuth)".into(),
            provider: ProviderKind::CodexOAuth,
            context_window: 400_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gpt-5.4-mini".into(),
            display_name: "GPT-5.4 Mini Codex (OAuth)".into(),
            provider: ProviderKind::CodexOAuth,
            context_window: 400_000,
            default_max_tokens: 64_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "claude-opus-4-20250514".into(),
            display_name: "Claude Opus 4".into(),
            provider: ProviderKind::Anthropic,
            context_window: 200_000,
            default_max_tokens: 16_384,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "claude-haiku-4-5-20251001".into(),
            display_name: "Claude Haiku 4.5".into(),
            provider: ProviderKind::Anthropic,
            context_window: 200_000,
            default_max_tokens: 8_192,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: true,
                supports_tools: true,
            },
        },
        // ── OpenAI ──
        ModelInfo {
            id: "gpt-4.1".into(),
            display_name: "GPT-4.1".into(),
            provider: ProviderKind::OpenAI,
            context_window: 1_047_576,
            default_max_tokens: 32_768,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gpt-4.1-mini".into(),
            display_name: "GPT-4.1 Mini".into(),
            provider: ProviderKind::OpenAI,
            context_window: 1_047_576,
            default_max_tokens: 16_384,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "o4-mini".into(),
            display_name: "o4-mini".into(),
            provider: ProviderKind::OpenAI,
            context_window: 200_000,
            default_max_tokens: 100_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        // ── Gemini ──
        ModelInfo {
            id: "gemini-2.5-pro".into(),
            display_name: "Gemini 2.5 Pro".into(),
            provider: ProviderKind::Gemini,
            context_window: 1_048_576,
            default_max_tokens: 65_536,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "gemini-2.5-flash".into(),
            display_name: "Gemini 2.5 Flash".into(),
            provider: ProviderKind::Gemini,
            context_window: 1_048_576,
            default_max_tokens: 65_536,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        // ── Xiaomi MiMo ──
        ModelInfo {
            id: "mimo-v2.5-pro".into(),
            display_name: "MiMo V2.5 Pro".into(),
            provider: ProviderKind::Mimo,
            context_window: 1_000_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: false,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "mimo-v2.5".into(),
            display_name: "MiMo V2.5".into(),
            provider: ProviderKind::Mimo,
            context_window: 1_000_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "mimo-v2-flash".into(),
            display_name: "MiMo V2 Flash".into(),
            provider: ProviderKind::Mimo,
            context_window: 256_000,
            default_max_tokens: 64_000,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: false,
                supports_tools: true,
            },
        },
        // ── DeepSeek ──
        ModelInfo {
            id: "deepseek-v4-pro".into(),
            display_name: "DeepSeek V4 Pro".into(),
            provider: ProviderKind::DeepSeek,
            context_window: 1_000_000,
            default_max_tokens: 16_384,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: false,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "deepseek-v4-flash".into(),
            display_name: "DeepSeek V4 Flash".into(),
            provider: ProviderKind::DeepSeek,
            context_window: 1_000_000,
            default_max_tokens: 16_384,
            capabilities: ModelCapabilities {
                can_reason: true,
                supports_vision: false,
                supports_tools: true,
            },
        },
        // ── GLM/Zhipu AI ──
        ModelInfo {
            id: "glm-5.1".into(),
            display_name: "GLM-5.1".into(),
            provider: ProviderKind::Glm,
            context_window: 200_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: false,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "glm-4.7".into(),
            display_name: "GLM-4.7".into(),
            provider: ProviderKind::Glm,
            context_window: 200_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: false,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "glm-4.7-flash".into(),
            display_name: "GLM-4.7 Flash (Free)".into(),
            provider: ProviderKind::Glm,
            context_window: 200_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: false,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "glm-4.6".into(),
            display_name: "GLM-4.6".into(),
            provider: ProviderKind::Glm,
            context_window: 200_000,
            default_max_tokens: 128_000,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: true,
                supports_tools: true,
            },
        },
        ModelInfo {
            id: "glm-4.5-air".into(),
            display_name: "GLM-4.5 Air".into(),
            provider: ProviderKind::Glm,
            context_window: 128_000,
            default_max_tokens: 96_000,
            capabilities: ModelCapabilities {
                can_reason: false,
                supports_vision: false,
                supports_tools: true,
            },
        },
    ]
}

// ─── Detection ────────────────────────────────────────────────────────────────

/// Configuration needed to detect models from a provider.
#[derive(Clone, Debug)]
pub struct ProviderConfig {
    /// API key (required for cloud providers).
    pub api_key: Option<String>,
    /// Custom endpoint URL (optional override).
    pub endpoint: Option<String>,
}

/// Attempts to discover available models by calling the provider's list API.
///
/// Returns a list of detected models on success, or an error if the API
/// is unreachable or the key is invalid.
pub fn detect_available_models(
    provider: &ProviderKind,
    config: &ProviderConfig,
) -> EngineResult<Vec<ModelInfo>> {
    match provider {
        ProviderKind::Anthropic => detect_anthropic(config),
        ProviderKind::OpenAI => detect_openai(config),
        ProviderKind::CodexOAuth => Ok(ModelRegistry::new()
            .builtin_for(&ProviderKind::CodexOAuth)
            .into_iter()
            .cloned()
            .collect()),
        ProviderKind::Gemini => detect_gemini(config),
        ProviderKind::Ollama => detect_ollama(config),
        ProviderKind::Custom => detect_openai_compatible(config),
        ProviderKind::Mimo | ProviderKind::DeepSeek | ProviderKind::Glm => {
            detect_openai_compatible_typed(provider, config)
        }
    }
}

fn detect_anthropic(config: &ProviderConfig) -> EngineResult<Vec<ModelInfo>> {
    let api_key = config
        .api_key
        .as_deref()
        .ok_or_else(|| EngineError::config("Anthropic API key required for model detection"))?;

    let endpoint = config
        .endpoint
        .as_deref()
        .unwrap_or("https://api.anthropic.com");

    let url = format!("{endpoint}/v1/models");
    let mut response = ureq::get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .call()
        .map_err(|e| EngineError::other(format!("Anthropic models API failed: {e}")))?;

    let json: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| EngineError::other(format!("Anthropic models response parse failed: {e}")))?;

    let models = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let id = entry["id"].as_str()?;
                    let name = entry["display_name"].as_str().unwrap_or(id);
                    Some(ModelInfo {
                        id: id.to_owned(),
                        display_name: name.to_owned(),
                        provider: ProviderKind::Anthropic,
                        context_window: 200_000,
                        default_max_tokens: 16_384,
                        capabilities: ModelCapabilities {
                            can_reason: id.contains("opus") || id.contains("sonnet"),
                            supports_vision: true,
                            supports_tools: true,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

fn detect_openai(config: &ProviderConfig) -> EngineResult<Vec<ModelInfo>> {
    let api_key = config
        .api_key
        .as_deref()
        .ok_or_else(|| EngineError::config("OpenAI API key required for model detection"))?;

    let endpoint = config
        .endpoint
        .as_deref()
        .unwrap_or("https://api.openai.com/v1");

    let url = format!("{endpoint}/models");
    let mut response = ureq::get(&url)
        .header("Authorization", &format!("Bearer {api_key}"))
        .call()
        .map_err(|e| EngineError::other(format!("OpenAI models API failed: {e}")))?;

    let json: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| EngineError::other(format!("OpenAI models response parse failed: {e}")))?;

    let models = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let id = entry["id"].as_str()?;
                    // Filter to chat-capable models only
                    if !is_openai_chat_model(id) {
                        return None;
                    }
                    Some(ModelInfo {
                        id: id.to_owned(),
                        display_name: id.to_owned(),
                        provider: ProviderKind::OpenAI,
                        context_window: 128_000,
                        default_max_tokens: 16_384,
                        capabilities: ModelCapabilities {
                            can_reason: id.starts_with("o1")
                                || id.starts_with("o3")
                                || id.starts_with("o4"),
                            supports_vision: true,
                            supports_tools: true,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Filters OpenAI model IDs to chat-capable models.
fn is_openai_chat_model(id: &str) -> bool {
    let chat_prefixes = ["gpt-", "o1", "o3", "o4", "chatgpt"];
    chat_prefixes.iter().any(|prefix| id.starts_with(prefix))
        && !id.contains("instruct")
        && !id.contains("realtime")
        && !id.contains("audio")
        && !id.contains("image")
        && !id.contains("tts")
        && !id.contains("whisper")
        && !id.contains("dall-e")
        && !id.contains("embedding")
}

fn detect_gemini(config: &ProviderConfig) -> EngineResult<Vec<ModelInfo>> {
    let api_key = config
        .api_key
        .as_deref()
        .ok_or_else(|| EngineError::config("Gemini API key required for model detection"))?;

    let endpoint = config
        .endpoint
        .as_deref()
        .unwrap_or("https://generativelanguage.googleapis.com");

    let url = format!("{endpoint}/v1beta/models?key={api_key}");
    let mut response = ureq::get(&url)
        .call()
        .map_err(|e| EngineError::other(format!("Gemini models API failed: {e}")))?;

    let json: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| EngineError::other(format!("Gemini models response parse failed: {e}")))?;

    let models = json["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let name = entry["name"].as_str()?;
                    // name is like "models/gemini-2.5-pro"
                    let id = name.strip_prefix("models/").unwrap_or(name);
                    // Only include generateContent-capable models
                    let methods = entry["supportedGenerationMethods"].as_array()?;
                    let can_generate = methods
                        .iter()
                        .any(|m| m.as_str() == Some("generateContent"));
                    if !can_generate {
                        return None;
                    }
                    let display = entry["displayName"].as_str().unwrap_or(id);
                    let input_limit = entry["inputTokenLimit"].as_u64().unwrap_or(1_048_576) as u32;
                    let output_limit = entry["outputTokenLimit"].as_u64().unwrap_or(65_536) as u32;

                    Some(ModelInfo {
                        id: id.to_owned(),
                        display_name: display.to_owned(),
                        provider: ProviderKind::Gemini,
                        context_window: input_limit,
                        default_max_tokens: output_limit,
                        capabilities: ModelCapabilities {
                            can_reason: id.contains("pro") || id.contains("think"),
                            supports_vision: true,
                            supports_tools: true,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

fn detect_ollama(config: &ProviderConfig) -> EngineResult<Vec<ModelInfo>> {
    let endpoint = config
        .endpoint
        .as_deref()
        .unwrap_or("http://localhost:11434");

    let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
    let mut response = ureq::get(&url).call().map_err(|e| {
        EngineError::other(format!(
            "Ollama API failed (is the server running at {endpoint}?): {e}"
        ))
    })?;

    let json: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| EngineError::other(format!("Ollama response parse failed: {e}")))?;

    let models = json["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let name = entry["name"].as_str()?;
                    let size = entry["size"].as_u64().unwrap_or(0);
                    // Rough heuristic: larger models generally have bigger context
                    let context_window = if size > 10_000_000_000 {
                        128_000
                    } else {
                        8_192
                    };
                    Some(ModelInfo {
                        id: name.to_owned(),
                        display_name: name.to_owned(),
                        provider: ProviderKind::Ollama,
                        context_window,
                        default_max_tokens: 4_096,
                        capabilities: ModelCapabilities {
                            can_reason: name.contains("qwq")
                                || name.contains("deepseek-r1")
                                || name.contains("think"),
                            supports_vision: name.contains("llava")
                                || name.contains("vision")
                                || name.contains("bakllava"),
                            supports_tools: false,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

fn detect_openai_compatible(config: &ProviderConfig) -> EngineResult<Vec<ModelInfo>> {
    let endpoint = config.endpoint.as_deref().ok_or_else(|| {
        EngineError::config("Custom endpoint URL is required for model detection")
    })?;

    let url = format!("{}/models", endpoint.trim_end_matches('/'));
    let mut request = ureq::get(&url);
    if let Some(key) = config.api_key.as_deref() {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }

    let mut response = request
        .call()
        .map_err(|e| EngineError::other(format!("Custom endpoint models API failed: {e}")))?;

    let json: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| EngineError::other(format!("Custom endpoint response parse failed: {e}")))?;

    let models = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let id = entry["id"].as_str()?;
                    Some(ModelInfo {
                        id: id.to_owned(),
                        display_name: id.to_owned(),
                        provider: ProviderKind::Custom,
                        context_window: 128_000,
                        default_max_tokens: 4_096,
                        capabilities: ModelCapabilities::default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Detects models from an OpenAI-compatible API and tags them with the given provider kind.
fn detect_openai_compatible_typed(
    provider: &ProviderKind,
    config: &ProviderConfig,
) -> EngineResult<Vec<ModelInfo>> {
    let endpoint = config
        .endpoint
        .as_deref()
        .or_else(|| provider.default_endpoint())
        .ok_or_else(|| EngineError::config("Endpoint URL is required for model detection"))?;

    let url = format!("{}/models", endpoint.trim_end_matches('/'));
    let mut request = ureq::get(&url);
    if let Some(key) = config.api_key.as_deref() {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }

    let mut response = request.call().map_err(|e| {
        EngineError::other(format!(
            "{} models API failed: {e}",
            provider.display_name()
        ))
    })?;

    let json: serde_json::Value = response.body_mut().read_json().map_err(|e| {
        EngineError::other(format!(
            "{} models response parse failed: {e}",
            provider.display_name()
        ))
    })?;

    let models = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let id = entry["id"].as_str()?;
                    Some(ModelInfo {
                        id: id.to_owned(),
                        display_name: entry["display_name"].as_str().unwrap_or(id).to_owned(),
                        provider: provider.clone(),
                        context_window: 128_000,
                        default_max_tokens: 4_096,
                        capabilities: ModelCapabilities {
                            can_reason: id.contains("reason") || id.contains("think"),
                            supports_vision: id.contains("vision"),
                            supports_tools: true,
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::{
        detect_available_models, is_openai_chat_model, GlmEndpoints, MimoEndpoints, ProviderConfig,
        ProviderKind,
    };
    use engine_editor::{BillingMode, GlmRegion, MimoRegion};
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn openai_chat_filter_accepts_current_gpt_families() {
        assert!(is_openai_chat_model("gpt-5"));
        assert!(is_openai_chat_model("gpt-5-mini"));
        assert!(is_openai_chat_model("gpt-4.1"));
        assert!(is_openai_chat_model("o3"));
    }

    #[test]
    fn openai_chat_filter_rejects_non_chat_models() {
        assert!(!is_openai_chat_model("gpt-image-1"));
        assert!(!is_openai_chat_model("gpt-4o-realtime-preview"));
        assert!(!is_openai_chat_model("text-embedding-3-large"));
    }

    #[test]
    fn mimo_endpoints_follow_billing_and_subscription_region() {
        assert_eq!(
            MimoEndpoints::base_url(&BillingMode::Subscription, &MimoRegion::China),
            "https://token-plan-cn.xiaomimimo.com/v1"
        );
        assert_eq!(
            MimoEndpoints::base_url(&BillingMode::Subscription, &MimoRegion::Singapore),
            "https://token-plan-sgp.xiaomimimo.com/v1"
        );
        assert_eq!(
            MimoEndpoints::base_url(&BillingMode::Subscription, &MimoRegion::Europe),
            "https://token-plan-ams.xiaomimimo.com/v1"
        );
        assert_eq!(
            MimoEndpoints::base_url(&BillingMode::Api, &MimoRegion::Europe),
            "https://api.xiaomimimo.com/v1"
        );
    }

    #[test]
    fn glm_endpoints_follow_billing_and_region() {
        assert_eq!(
            GlmEndpoints::base_url(&BillingMode::Subscription, &GlmRegion::Bigmodel),
            "https://open.bigmodel.cn/api/coding/paas/v4"
        );
        assert_eq!(
            GlmEndpoints::base_url(&BillingMode::Subscription, &GlmRegion::Zai),
            "https://api.z.ai/api/coding/paas/v4"
        );
        assert_eq!(
            GlmEndpoints::base_url(&BillingMode::Api, &GlmRegion::Bigmodel),
            "https://open.bigmodel.cn/api/paas/v4"
        );
        assert_eq!(
            GlmEndpoints::base_url(&BillingMode::Api, &GlmRegion::Zai),
            "https://api.z.ai/api/paas/v4"
        );
    }

    #[test]
    fn openai_detection_returns_provider_gpt5_models() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let body = r#"{"data":[{"id":"gpt-5"},{"id":"gpt-image-1"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let models = detect_available_models(
            &ProviderKind::OpenAI,
            &ProviderConfig {
                api_key: Some("test-key".to_owned()),
                endpoint: Some(format!("http://{address}")),
            },
        )
        .unwrap();
        server.join().unwrap();

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5");
    }
}

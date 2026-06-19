//! Real AI model provider implementations for the Copilot.
//!
//! Supports Anthropic Claude (via Messages API), OpenAI-compatible APIs,
//! Google Gemini, and local Ollama instances.

use crate::{
    AiModel, AiRequest, AiResponse, AiStreamDelta, ChatMessage, ChatRole, ToolCall, ToolCallDelta,
};
use engine_core::{EngineError, EngineResult};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};

type DeltaExtractor = for<'a> fn(&'a serde_json::Value) -> Option<&'a str>;

fn stream_json_lines(
    reader: impl Read,
    sse: bool,
    provider: &str,
    extract_text: DeltaExtractor,
    extract_thinking: Option<DeltaExtractor>,
    on_delta: &mut dyn FnMut(AiStreamDelta),
) -> EngineResult<AiResponse> {
    let mut content = String::new();
    let mut thinking = String::new();

    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|error| {
            EngineError::other(format!(
                "{provider} streaming response read failed: {error}"
            ))
        })?;
        let payload = if sse {
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            payload.trim()
        } else {
            line.trim()
        };

        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let json: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
            EngineError::other(format!(
                "{provider} streaming response parse failed: {error}; payload: {payload}"
            ))
        })?;
        if let Some(message) = json["error"]["message"].as_str() {
            return Err(EngineError::other(format!("{provider} API: {message}")));
        }
        if json["type"] == "response.failed" {
            let message = json["response"]["error"]["message"]
                .as_str()
                .unwrap_or("streaming response failed");
            return Err(EngineError::other(format!("{provider} API: {message}")));
        }
        if let Some(delta) = extract_thinking.and_then(|extract| extract(&json)) {
            thinking.push_str(delta);
            on_delta(AiStreamDelta::Thinking(delta.to_owned()));
        }
        if let Some(delta) = extract_text(&json) {
            content.push_str(delta);
            on_delta(AiStreamDelta::Text(delta.to_owned()));
        }
    }

    Ok(AiResponse {
        content,
        thinking,
        tool_calls: Vec::new(),
    })
}

/// Stream handler that extracts both thinking and text content from Anthropic responses.
fn stream_anthropic_with_thinking(
    reader: impl Read,
    on_delta: &mut dyn FnMut(AiStreamDelta),
) -> EngineResult<AiResponse> {
    let mut content = String::new();
    let mut thinking_content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    // Track in-progress tool calls by index
    let mut active_tools: HashMap<usize, (String, String, String)> = HashMap::new();

    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|error| {
            EngineError::other(format!("Anthropic streaming response read failed: {error}"))
        })?;
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();

        if payload.is_empty() {
            continue;
        }

        let json: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
            EngineError::other(format!(
                "Anthropic streaming response parse failed: {error}; payload: {payload}"
            ))
        })?;
        if let Some(message) = json["error"]["message"].as_str() {
            return Err(EngineError::other(format!("Anthropic API: {message}")));
        }

        let event_type = json["type"].as_str().unwrap_or("");
        match event_type {
            "content_block_start" => {
                let index = json["index"].as_u64().unwrap_or(0) as usize;
                let block = &json["content_block"];
                if block["type"] == "tool_use" {
                    let id = block["id"].as_str().unwrap_or("").to_owned();
                    let name = block["name"].as_str().unwrap_or("").to_owned();
                    active_tools.insert(index, (id, name.clone(), String::new()));
                    on_delta(AiStreamDelta::ToolCallDelta(ToolCallDelta {
                        id: String::new(),
                        name,
                        arguments_delta: String::new(),
                    }));
                }
            }
            "content_block_delta" => {
                let index = json["index"].as_u64().unwrap_or(0) as usize;
                let delta_type = json["delta"]["type"].as_str().unwrap_or("");
                if delta_type == "thinking_delta" {
                    if let Some(thinking) = json["delta"]["thinking"].as_str() {
                        thinking_content.push_str(thinking);
                        on_delta(AiStreamDelta::Thinking(thinking.to_owned()));
                    }
                } else if delta_type == "text_delta" {
                    if let Some(text) = json["delta"]["text"].as_str() {
                        content.push_str(text);
                        on_delta(AiStreamDelta::Text(text.to_owned()));
                    }
                } else if delta_type == "input_json_delta" {
                    if let Some(partial) = json["delta"]["partial_json"].as_str() {
                        if let Some((_, _, args)) = active_tools.get_mut(&index) {
                            args.push_str(partial);
                            on_delta(AiStreamDelta::ToolCallDelta(ToolCallDelta {
                                id: String::new(),
                                name: String::new(),
                                arguments_delta: partial.to_owned(),
                            }));
                        }
                    }
                }
            }
            "content_block_stop" => {
                let index = json["index"].as_u64().unwrap_or(0) as usize;
                if let Some((id, name, args_json)) = active_tools.remove(&index) {
                    let arguments = serde_json::from_str(&args_json)
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
            }
            _ => {}
        }
    }

    Ok(AiResponse {
        content,
        thinking: thinking_content,
        tool_calls,
    })
}

fn anthropic_delta(json: &serde_json::Value) -> Option<&str> {
    json["delta"]["text"].as_str()
}

/// Stream handler for OpenAI Chat Completions with tool call support.
fn stream_openai_chat_completions(
    reader: impl Read,
    on_delta: &mut dyn FnMut(AiStreamDelta),
) -> EngineResult<AiResponse> {
    let mut content = String::new();
    let mut thinking = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    // Accumulate tool call fragments by index: (id, name, arguments_json)
    let mut active_tools: HashMap<usize, (String, String, String)> = HashMap::new();

    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|error| {
            EngineError::other(format!("OpenAI streaming response read failed: {error}"))
        })?;
        let Some(payload) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();

        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let json: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
            EngineError::other(format!(
                "OpenAI streaming response parse failed: {error}; payload: {payload}"
            ))
        })?;
        if let Some(message) = json["error"]["message"].as_str() {
            return Err(EngineError::other(format!("OpenAI API: {message}")));
        }

        // Extract thinking/reasoning content
        if let Some(rc) = json["choices"][0]["delta"]["reasoning_content"].as_str() {
            thinking.push_str(rc);
            on_delta(AiStreamDelta::Thinking(rc.to_owned()));
        } else if let Some(rc) = json["choices"][0]["delta"]["reasoning"].as_str() {
            thinking.push_str(rc);
            on_delta(AiStreamDelta::Thinking(rc.to_owned()));
        }

        // Extract text content
        if let Some(text) = json["choices"][0]["delta"]["content"].as_str() {
            content.push_str(text);
            on_delta(AiStreamDelta::Text(text.to_owned()));
        }

        // Extract tool call deltas
        if let Some(calls) = json["choices"][0]["delta"]["tool_calls"].as_array() {
            for tc in calls {
                let index = tc["index"].as_u64().unwrap_or(0) as usize;
                if let Some(id) = tc["id"].as_str() {
                    // First chunk: new tool call with id and name
                    let name = tc["function"]["name"].as_str().unwrap_or("").to_owned();
                    active_tools.insert(index, (id.to_owned(), name.clone(), String::new()));
                    on_delta(AiStreamDelta::ToolCallDelta(ToolCallDelta {
                        id: id.to_owned(),
                        name,
                        arguments_delta: String::new(),
                    }));
                }
                if let Some(args_delta) = tc["function"]["arguments"].as_str() {
                    if let Some((_, _, args)) = active_tools.get_mut(&index) {
                        args.push_str(args_delta);
                        on_delta(AiStreamDelta::ToolCallDelta(ToolCallDelta {
                            id: String::new(),
                            name: String::new(),
                            arguments_delta: args_delta.to_owned(),
                        }));
                    }
                }
            }
        }

        // Check for finish_reason to finalize tool calls
        if let Some(reason) = json["choices"][0]["finish_reason"].as_str() {
            if reason == "tool_calls" || reason == "stop" {
                for (_, (id, name, args_json)) in active_tools.drain() {
                    let arguments = serde_json::from_str(&args_json)
                        .unwrap_or(serde_json::Value::Object(Default::default()));
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
            }
        }
    }

    // Finalize any remaining tool calls (in case finish_reason wasn't received)
    for (_, (id, name, args_json)) in active_tools {
        let arguments = serde_json::from_str(&args_json)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        tool_calls.push(ToolCall {
            id,
            name,
            arguments,
        });
    }

    Ok(AiResponse {
        content,
        thinking,
        tool_calls,
    })
}

fn openai_delta(json: &serde_json::Value) -> Option<&str> {
    json["choices"][0]["delta"]["content"].as_str()
}

fn openai_thinking_delta(json: &serde_json::Value) -> Option<&str> {
    json["choices"][0]["delta"]["reasoning_content"]
        .as_str()
        .or_else(|| json["choices"][0]["delta"]["reasoning"].as_str())
}

fn ollama_delta(json: &serde_json::Value) -> Option<&str> {
    json["message"]["content"].as_str()
}

fn ollama_thinking_delta(json: &serde_json::Value) -> Option<&str> {
    json["message"]["thinking"].as_str()
}

fn gemini_delta(json: &serde_json::Value) -> Option<&str> {
    json["candidates"][0]["content"]["parts"]
        .as_array()?
        .iter()
        .find(|part| !part["thought"].as_bool().unwrap_or(false))?["text"]
        .as_str()
}

fn gemini_thinking_delta(json: &serde_json::Value) -> Option<&str> {
    json["candidates"][0]["content"]["parts"]
        .as_array()?
        .iter()
        .find(|part| part["thought"].as_bool().unwrap_or(false))?["text"]
        .as_str()
}

fn codex_delta(json: &serde_json::Value) -> Option<&str> {
    if json["type"] == "response.output_text.delta" {
        json["delta"].as_str()
    } else {
        None
    }
}

fn codex_thinking_delta(json: &serde_json::Value) -> Option<&str> {
    if json["type"] == "response.reasoning_summary_text.delta" {
        json["delta"].as_str()
    } else {
        None
    }
}

fn messages_to_responses_input(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter(|message| message.role != ChatRole::System)
        .map(|message| {
            let content_type = if message.role == ChatRole::Assistant {
                "output_text"
            } else {
                "input_text"
            };
            serde_json::json!({
                "type": "message",
                "role": match message.role {
                    ChatRole::Assistant => "assistant",
                    ChatRole::User | ChatRole::System => "user",
                },
                "content": [{ "type": content_type, "text": message.content }]
            })
        })
        .collect()
}

/// Short-lived credentials used to call the ChatGPT Codex backend.
#[derive(Clone, Debug)]
pub struct CodexOAuthCredentials {
    /// OAuth access token.
    pub access_token: String,
    /// ChatGPT account identifier extracted from the OAuth token.
    pub account_id: Option<String>,
}

/// OpenAI Codex provider backed by a ChatGPT OAuth subscription.
pub struct CodexOAuthProvider {
    credentials: CodexOAuthCredentials,
    model: String,
    endpoint: String,
    max_tokens: u32,
}

impl CodexOAuthProvider {
    /// Creates a Codex OAuth provider.
    pub fn new(
        model: &str,
        credentials: CodexOAuthCredentials,
        endpoint: Option<&str>,
        max_tokens: u32,
    ) -> Self {
        Self {
            credentials,
            model: model.to_owned(),
            endpoint: endpoint
                .unwrap_or("https://chatgpt.com/backend-api/codex/responses")
                .trim_end_matches('/')
                .to_owned(),
            max_tokens,
        }
    }
}

impl AiModel for CodexOAuthProvider {
    fn chat(&self, request: AiRequest) -> EngineResult<AiResponse> {
        self.chat_stream(request, &mut |_| {})
    }

    fn chat_stream(
        &self,
        request: AiRequest,
        on_delta: &mut dyn FnMut(AiStreamDelta),
    ) -> EngineResult<AiResponse> {
        let input = messages_to_responses_input(&request.messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "instructions": request.system,
            "input": input,
            "max_output_tokens": self.max_tokens,
            "store": false,
            "stream": true,
            "include": ["reasoning.encrypted_content"]
        });

        if let Some(thinking_effort) = &request.thinking_effort {
            use crate::ThinkingEffort;
            let effort = match thinking_effort {
                ThinkingEffort::Off => "none",
                ThinkingEffort::Low => "low",
                ThinkingEffort::Medium => "medium",
                ThinkingEffort::High => "high",
            };
            body["reasoning"] = serde_json::json!({
                "effort": effort,
                "summary": "auto"
            });
        }

        let mut request_builder = ureq::post(&self.endpoint)
            .header(
                "Authorization",
                &format!("Bearer {}", self.credentials.access_token),
            )
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("originator", "aster")
            .header("User-Agent", concat!("aster/", env!("CARGO_PKG_VERSION")));
        if let Some(account_id) = &self.credentials.account_id {
            request_builder = request_builder.header("ChatGPT-Account-Id", account_id);
        }

        let mut response = request_builder
            .send_json(&body)
            .map_err(|error| EngineError::other(format!("Codex API request failed: {error}")))?;

        stream_json_lines(
            response.body_mut().as_reader(),
            true,
            "Codex",
            codex_delta,
            Some(codex_thinking_delta),
            on_delta,
        )
    }
}

/// Converts a slice of `ChatMessage` into the JSON array format used by
/// OpenAI-compatible and Ollama APIs: `[{"role": "...", "content": "..."}]`.
fn messages_to_openai_json(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|msg| {
            let role = match msg.role {
                ChatRole::System => "system",
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
            };
            serde_json::json!({ "role": role, "content": msg.content })
        })
        .collect()
}

/// Converts a slice of `ChatMessage` into the Anthropic Messages API format.
/// System messages are filtered out (Anthropic uses a top-level `system` field).
fn messages_to_anthropic_json(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter(|msg| msg.role != ChatRole::System)
        .map(|msg| {
            let role = match msg.role {
                ChatRole::User | ChatRole::System => "user",
                ChatRole::Assistant => "assistant",
            };
            serde_json::json!({ "role": role, "content": msg.content })
        })
        .collect()
}

// ─── Anthropic Provider ────────────────────────────────────────────────────

/// Anthropic Claude provider using the Messages API.
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl AnthropicProvider {
    /// Creates a new Anthropic provider.
    ///
    /// # Errors
    /// Returns `EngineError::Config` if `api_key` is empty.
    pub fn new(api_key: &str, model: &str, max_tokens: u32) -> EngineResult<Self> {
        if api_key.is_empty() {
            return Err(EngineError::config(
                "Anthropic API key is not configured. Set it in the editor settings.",
            ));
        }
        Ok(Self {
            api_key: api_key.to_owned(),
            model: model.to_owned(),
            max_tokens,
        })
    }
}

impl AiModel for AnthropicProvider {
    fn chat(&self, request: AiRequest) -> EngineResult<AiResponse> {
        self.chat_stream(request, &mut |_| {})
    }

    fn chat_stream(
        &self,
        request: AiRequest,
        on_delta: &mut dyn FnMut(AiStreamDelta),
    ) -> EngineResult<AiResponse> {
        let messages = messages_to_anthropic_json(&request.messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": request.system,
            "messages": messages,
            "stream": true
        });

        // Add thinking configuration if requested
        if let Some(thinking_effort) = &request.thinking_effort {
            use crate::ThinkingEffort;
            let budget_tokens = match thinking_effort {
                ThinkingEffort::Off => 0,
                ThinkingEffort::Low => 4096,
                ThinkingEffort::Medium => 10000,
                ThinkingEffort::High => 32000,
            };
            if budget_tokens > 0 {
                body["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": budget_tokens
                });
                // Anthropic requires max_tokens > budget_tokens
                if self.max_tokens <= budget_tokens {
                    body["max_tokens"] = serde_json::json!(budget_tokens + 4096);
                }
            }
        }

        // Add tool definitions if provided
        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|tool| {
                    serde_json::json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        let mut response = ureq::post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| EngineError::other(format!("Anthropic API request failed: {e}")))?;

        if request.thinking_effort.is_some() || !request.tools.is_empty() {
            stream_anthropic_with_thinking(response.body_mut().as_reader(), on_delta)
        } else {
            stream_json_lines(
                response.body_mut().as_reader(),
                true,
                "Anthropic",
                anthropic_delta,
                None,
                on_delta,
            )
        }
    }
}

// ─── Ollama Provider ────────────────────────────────────────────────────────

/// Local Ollama provider.
pub struct OllamaProvider {
    endpoint: String,
    model: String,
}

impl OllamaProvider {
    /// Creates a new Ollama provider.
    ///
    /// `endpoint` defaults to `http://localhost:11434` if `None`.
    pub fn new(model: &str, endpoint: Option<&str>) -> Self {
        let endpoint = endpoint
            .unwrap_or("http://localhost:11434")
            .trim_end_matches('/')
            .to_owned();
        Self {
            endpoint,
            model: model.to_owned(),
        }
    }
}

impl AiModel for OllamaProvider {
    fn chat(&self, request: AiRequest) -> EngineResult<AiResponse> {
        self.chat_stream(request, &mut |_| {})
    }

    fn chat_stream(
        &self,
        request: AiRequest,
        on_delta: &mut dyn FnMut(AiStreamDelta),
    ) -> EngineResult<AiResponse> {
        let url = format!("{}/api/chat", self.endpoint);

        // Prepend system message, then conversation history
        let mut all_messages =
            vec![serde_json::json!({"role": "system", "content": request.system})];
        all_messages.extend(messages_to_openai_json(&request.messages));

        let mut body = serde_json::json!({
            "model": self.model,
            "stream": true,
            "messages": all_messages
        });

        // Add think parameter if thinking is requested
        if let Some(thinking_effort) = &request.thinking_effort {
            use crate::ThinkingEffort;
            let think = !matches!(thinking_effort, ThinkingEffort::Off);
            body["think"] = serde_json::json!(think);
        }

        let mut response = ureq::post(&url)
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| {
                EngineError::other(format!(
                    "Ollama request failed (is the Ollama server running at {}?): {e}",
                    self.endpoint
                ))
            })?;

        stream_json_lines(
            response.body_mut().as_reader(),
            false,
            "Ollama",
            ollama_delta,
            Some(ollama_thinking_delta),
            on_delta,
        )
    }
}

// ─── OpenAI-compatible Provider ─────────────────────────────────────────────

/// Thinking mode format for OpenAI-compatible providers.
#[derive(Clone, Debug, Default)]
pub enum ThinkingFormat {
    /// OpenAI format: reasoning.effort
    #[default]
    OpenAI,
    /// MiMo/GLM format: thinking.type
    ThinkingType,
    /// DeepSeek format: thinking.type + reasoning_effort
    DeepSeek,
}

fn apply_thinking_config(
    body: &mut serde_json::Value,
    thinking_effort: &crate::ThinkingEffort,
    format: &ThinkingFormat,
) -> EngineResult<()> {
    use crate::ThinkingEffort;
    match format {
        ThinkingFormat::OpenAI => {
            let effort = match thinking_effort {
                ThinkingEffort::Off => return Err(EngineError::config(
                    "OpenAI reasoning models do not support disabling reasoning. Use 'low' effort instead."
                )),
                ThinkingEffort::Low => "low",
                ThinkingEffort::Medium => "medium",
                ThinkingEffort::High => "high",
            };
            body["reasoning"] = serde_json::json!({ "effort": effort });
        }
        ThinkingFormat::ThinkingType => {
            body["thinking"] = serde_json::json!({
                "type": if matches!(thinking_effort, ThinkingEffort::Off) {
                    "disabled"
                } else {
                    "enabled"
                }
            });
        }
        ThinkingFormat::DeepSeek => match thinking_effort {
            ThinkingEffort::Off => {
                body["thinking"] = serde_json::json!({ "type": "disabled" });
            }
            _ => {
                let effort = match thinking_effort {
                    ThinkingEffort::Low => "low",
                    ThinkingEffort::Medium => "medium",
                    ThinkingEffort::High => "high",
                    ThinkingEffort::Off => unreachable!(),
                };
                body["thinking"] = serde_json::json!({ "type": "enabled" });
                body["reasoning_effort"] = serde_json::json!(effort);
            }
        },
    }
    Ok(())
}

/// OpenAI-compatible provider (works with OpenAI, Groq, Together, etc.).
pub struct OpenAIProvider {
    api_key: String,
    model: String,
    endpoint: String,
    max_tokens: u32,
    thinking_format: ThinkingFormat,
    uses_responses_api: bool,
}

impl OpenAIProvider {
    /// Creates a new OpenAI-compatible provider.
    ///
    /// `endpoint` defaults to `https://api.openai.com/v1` if `None`.
    pub fn new(model: &str, api_key: &str, endpoint: Option<&str>, max_tokens: u32) -> Self {
        let endpoint = endpoint
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_owned();
        let uses_responses_api = endpoint.contains("api.openai.com");
        // Detect thinking format based on endpoint
        let thinking_format = if endpoint.contains("xiaomimimo.com") {
            ThinkingFormat::ThinkingType
        } else if endpoint.contains("deepseek.com") {
            ThinkingFormat::DeepSeek
        } else {
            ThinkingFormat::OpenAI
        };
        Self {
            api_key: api_key.to_owned(),
            model: model.to_owned(),
            endpoint,
            max_tokens,
            thinking_format,
            uses_responses_api,
        }
    }

    /// Creates a new OpenAI-compatible provider with explicit thinking format.
    pub fn new_with_format(
        model: &str,
        api_key: &str,
        endpoint: Option<&str>,
        max_tokens: u32,
        thinking_format: ThinkingFormat,
    ) -> Self {
        let endpoint = endpoint
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_owned();
        let uses_responses_api = endpoint.contains("api.openai.com")
            && matches!(&thinking_format, ThinkingFormat::OpenAI);
        Self {
            api_key: api_key.to_owned(),
            model: model.to_owned(),
            endpoint,
            max_tokens,
            thinking_format,
            uses_responses_api,
        }
    }
}

impl AiModel for OpenAIProvider {
    fn chat(&self, request: AiRequest) -> EngineResult<AiResponse> {
        self.chat_stream(request, &mut |_| {})
    }

    fn chat_stream(
        &self,
        request: AiRequest,
        on_delta: &mut dyn FnMut(AiStreamDelta),
    ) -> EngineResult<AiResponse> {
        if self.uses_responses_api {
            let url = format!("{}/responses", self.endpoint);
            let input = messages_to_responses_input(&request.messages);
            let mut body = serde_json::json!({
                "model": self.model,
                "instructions": request.system,
                "input": input,
                "max_output_tokens": self.max_tokens,
                "store": false,
                "stream": true,
                "include": ["reasoning.encrypted_content"]
            });

            if let Some(thinking_effort) = &request.thinking_effort {
                use crate::ThinkingEffort;
                let effort = match thinking_effort {
                    ThinkingEffort::Off => "none",
                    ThinkingEffort::Low => "low",
                    ThinkingEffort::Medium => "medium",
                    ThinkingEffort::High => "high",
                };
                body["reasoning"] = serde_json::json!({
                    "effort": effort,
                    "summary": "auto"
                });
            }

            let mut response = ureq::post(&url)
                .header("Authorization", &format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .header("Accept", "text/event-stream")
                .send_json(&body)
                .map_err(|e| EngineError::other(format!("OpenAI API request failed: {e}")))?;

            return stream_json_lines(
                response.body_mut().as_reader(),
                true,
                "OpenAI",
                codex_delta,
                Some(codex_thinking_delta),
                on_delta,
            );
        }

        let url = format!("{}/chat/completions", self.endpoint);

        // Prepend system message, then conversation history
        let mut all_messages =
            vec![serde_json::json!({"role": "system", "content": request.system})];
        all_messages.extend(messages_to_openai_json(&request.messages));

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": all_messages,
            "stream": true
        });

        // Add thinking/reasoning configuration based on provider format
        if let Some(thinking_effort) = &request.thinking_effort {
            apply_thinking_config(&mut body, thinking_effort, &self.thinking_format)?;
        }

        // Add tool definitions if provided
        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|tool| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        let mut response = ureq::post(&url)
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| EngineError::other(format!("OpenAI API request failed: {e}")))?;

        if !request.tools.is_empty() {
            stream_openai_chat_completions(response.body_mut().as_reader(), on_delta)
        } else {
            stream_json_lines(
                response.body_mut().as_reader(),
                true,
                "OpenAI",
                openai_delta,
                Some(openai_thinking_delta),
                on_delta,
            )
        }
    }
}

// ─── Gemini Provider ──────────────────────────────────────────────────────────

/// Google Gemini provider using the AI Studio / Vertex generateContent API.
pub struct GeminiProvider {
    api_key: String,
    model: String,
    endpoint: String,
    max_tokens: u32,
}

impl GeminiProvider {
    /// Creates a new Gemini provider.
    ///
    /// `endpoint` defaults to `https://generativelanguage.googleapis.com` if `None`.
    ///
    /// # Errors
    /// Returns `EngineError::Config` if `api_key` is empty.
    pub fn new(
        api_key: &str,
        model: &str,
        endpoint: Option<&str>,
        max_tokens: u32,
    ) -> EngineResult<Self> {
        if api_key.is_empty() {
            return Err(EngineError::config(
                "Gemini API key is not configured. Set it in the editor settings.",
            ));
        }
        let endpoint = endpoint
            .unwrap_or("https://generativelanguage.googleapis.com")
            .trim_end_matches('/')
            .to_owned();
        Ok(Self {
            api_key: api_key.to_owned(),
            model: model.to_owned(),
            endpoint,
            max_tokens,
        })
    }
}

impl AiModel for GeminiProvider {
    fn chat(&self, request: AiRequest) -> EngineResult<AiResponse> {
        self.chat_stream(request, &mut |_| {})
    }

    fn chat_stream(
        &self,
        request: AiRequest,
        on_delta: &mut dyn FnMut(AiStreamDelta),
    ) -> EngineResult<AiResponse> {
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            self.endpoint, self.model, self.api_key
        );

        // Convert messages to Gemini's contents format
        let contents: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|msg| msg.role != ChatRole::System)
            .map(|msg| {
                let role = match msg.role {
                    ChatRole::User | ChatRole::System => "user",
                    ChatRole::Assistant => "model",
                };
                serde_json::json!({
                    "role": role,
                    "parts": [{"text": msg.content}]
                })
            })
            .collect();

        let mut generation_config = serde_json::json!({
            "maxOutputTokens": self.max_tokens
        });

        // Add thinking configuration if requested
        if let Some(thinking_effort) = &request.thinking_effort {
            use crate::ThinkingEffort;
            let thinking_level = match thinking_effort {
                ThinkingEffort::Off => None,
                ThinkingEffort::Low => Some("low"),
                ThinkingEffort::Medium => Some("medium"),
                ThinkingEffort::High => Some("high"),
            };
            if let Some(level) = thinking_level {
                generation_config["thinkingConfig"] = serde_json::json!({
                    "thinkingLevel": level
                });
            }
        }

        let body = serde_json::json!({
            "system_instruction": {
                "parts": [{"text": request.system}]
            },
            "contents": contents,
            "generationConfig": generation_config
        });

        let mut response = ureq::post(&url)
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| EngineError::other(format!("Gemini API request failed: {e}")))?;

        stream_json_lines(
            response.body_mut().as_reader(),
            true,
            "Gemini",
            gemini_delta,
            Some(gemini_thinking_delta),
            on_delta,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_type_format_supports_enabling_and_disabling() {
        let mut enabled = serde_json::json!({});
        apply_thinking_config(
            &mut enabled,
            &crate::ThinkingEffort::High,
            &ThinkingFormat::ThinkingType,
        )
        .unwrap();
        assert_eq!(enabled["thinking"]["type"], "enabled");

        let mut disabled = serde_json::json!({});
        apply_thinking_config(
            &mut disabled,
            &crate::ThinkingEffort::Off,
            &ThinkingFormat::ThinkingType,
        )
        .unwrap();
        assert_eq!(disabled["thinking"]["type"], "disabled");
        assert!(disabled.get("reasoning").is_none());
    }

    #[test]
    fn parses_openai_sse_deltas() {
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut deltas = Vec::new();
        let response = stream_json_lines(
            input.as_bytes(),
            true,
            "OpenAI",
            openai_delta,
            Some(openai_thinking_delta),
            &mut |delta| deltas.push(delta),
        )
        .unwrap();

        assert_eq!(response.content, "hello");
        assert_eq!(response.thinking, "");
        assert_eq!(
            deltas,
            [
                AiStreamDelta::Text("hel".to_owned()),
                AiStreamDelta::Text("lo".to_owned()),
            ]
        );
    }

    #[test]
    fn separates_openai_compatible_reasoning_from_answer_text() {
        let input = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"considering\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut deltas = Vec::new();
        let response = stream_json_lines(
            input.as_bytes(),
            true,
            "OpenAI",
            openai_delta,
            Some(openai_thinking_delta),
            &mut |delta| deltas.push(delta),
        )
        .unwrap();

        assert_eq!(response.thinking, "considering");
        assert_eq!(response.content, "answer");
        assert_eq!(
            deltas,
            [
                AiStreamDelta::Thinking("considering".to_owned()),
                AiStreamDelta::Text("answer".to_owned()),
            ]
        );
    }

    #[test]
    fn parses_ollama_ndjson_deltas() {
        let input = concat!(
            "{\"message\":{\"content\":\"hel\"},\"done\":false}\n",
            "{\"message\":{\"content\":\"lo\"},\"done\":false}\n",
            "{\"message\":{\"content\":\"\"},\"done\":true}\n"
        );
        let response = stream_json_lines(
            input.as_bytes(),
            false,
            "Ollama",
            ollama_delta,
            Some(ollama_thinking_delta),
            &mut |_| {},
        )
        .unwrap();

        assert_eq!(response.content, "hello");
    }

    #[test]
    fn parses_codex_responses_sse_deltas() {
        let input = concat!(
            "event: response.reasoning_summary_text.delta\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"checking\"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n"
        );
        let response = stream_json_lines(
            input.as_bytes(),
            true,
            "Codex",
            codex_delta,
            Some(codex_thinking_delta),
            &mut |_| {},
        )
        .unwrap();

        assert_eq!(response.content, "hello");
        assert_eq!(response.thinking, "checking");
    }
}

// ─── Provider Factory ──────────────────────────────────────────────────────

/// Creates the appropriate `AiModel` implementation from a provider string,
/// model name, optional API key, optional endpoint, and max tokens.
///
/// Supported provider strings: `"anthropic"`, `"openai"`, `"codex_oauth"`,
/// `"gemini"`, `"ollama"`, `"custom"`, `"mimo"`, `"deepseek"`, and `"glm"`.
/// Returns `EngineError::Config` for unknown providers or missing API keys.
#[allow(clippy::module_name_repetitions)]
pub fn create_provider(
    provider: &str,
    model: &str,
    api_key: Option<&str>,
    endpoint: Option<&str>,
    max_tokens: u32,
    codex_oauth: Option<CodexOAuthCredentials>,
    mimo_config: Option<&engine_editor::MimoConfig>,
    glm_config: Option<&engine_editor::GlmConfig>,
) -> EngineResult<Box<dyn AiModel>> {
    match provider {
        "anthropic" => {
            let key = api_key.ok_or_else(|| {
                EngineError::config("Anthropic API key is required but not configured.")
            })?;
            Ok(Box::new(AnthropicProvider::new(key, model, max_tokens)?))
        }
        "openai" => {
            let key = api_key.ok_or_else(|| {
                EngineError::config("OpenAI API key is required but not configured.")
            })?;
            Ok(Box::new(OpenAIProvider::new(
                model, key, endpoint, max_tokens,
            )))
        }
        "codex_oauth" => {
            let credentials = codex_oauth.ok_or_else(|| {
                EngineError::config("Codex OAuth is not connected. Sign in with ChatGPT first.")
            })?;
            Ok(Box::new(CodexOAuthProvider::new(
                model,
                credentials,
                endpoint,
                max_tokens,
            )))
        }
        "gemini" => {
            let key = api_key.ok_or_else(|| {
                EngineError::config("Gemini API key is required but not configured.")
            })?;
            Ok(Box::new(GeminiProvider::new(
                key, model, endpoint, max_tokens,
            )?))
        }
        "ollama" => Ok(Box::new(OllamaProvider::new(model, endpoint))),
        "custom" => {
            // Custom uses OpenAI-compatible protocol with user-specified endpoint.
            let ep = endpoint.ok_or_else(|| {
                EngineError::config(
                    "Custom provider requires an endpoint URL. Set it in the editor settings.",
                )
            })?;
            Ok(Box::new(OpenAIProvider::new(
                model,
                api_key.unwrap_or(""),
                Some(ep),
                max_tokens,
            )))
        }
        "mimo" => {
            let config = mimo_config.ok_or_else(|| {
                EngineError::config("MiMo configuration is required.")
            })?;
            let ep = endpoint.unwrap_or_else(|| {
                crate::registry::MimoEndpoints::base_url(&config.billing, &config.region)
            });
            let key = api_key.ok_or_else(|| {
                EngineError::config("Xiaomi MiMo API key is required but not configured.")
            })?;
            Ok(Box::new(OpenAIProvider::new(model, key, Some(ep), max_tokens)))
        }
        "deepseek" => {
            let key = api_key.ok_or_else(|| {
                EngineError::config("DeepSeek API key is required but not configured.")
            })?;
            let ep = endpoint.unwrap_or("https://api.deepseek.com");
            Ok(Box::new(OpenAIProvider::new(model, key, Some(ep), max_tokens)))
        }
        "glm" => {
            let config = glm_config.ok_or_else(|| {
                EngineError::config("GLM configuration is required.")
            })?;
            let ep = endpoint.unwrap_or_else(|| {
                crate::registry::GlmEndpoints::base_url(&config.billing, &config.region)
            });
            let key = api_key.ok_or_else(|| {
                EngineError::config("GLM API key is required but not configured.")
            })?;
            Ok(Box::new(OpenAIProvider::new_with_format(
                model,
                key,
                Some(ep),
                max_tokens,
                ThinkingFormat::ThinkingType,
            )))
        }
        other => Err(EngineError::config(format!(
            "Unknown AI provider '{other}'. Supported: anthropic, openai, codex_oauth, gemini, ollama, custom, mimo, deepseek, glm"
        ))),
    }
}

use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;

use crate::models::InferenceConfig;
use crate::skills::executor::ToolDefinition;

/// Tokens sent over the channel.
/// ThinkingToken carries chain-of-thought content (prefixed with "\x01" sentinel).
/// Regular tokens carry visible response content.
pub const THINKING_PREFIX: &str = "\x01think:";

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    temperature: f32,
    top_p: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_prompt: Option<bool>,
    /// Qwen3-style thinking toggle via chat-template kwargs
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_template_kwargs: Option<serde_json::Map<String, JsonValue>>,
    /// Tool definitions (OpenAI function-calling format)
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    /// "auto" | "none" | {"type":"function","function":{"name":"..."}}
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<JsonValue>,
}

/// A chat message that may carry a raw JSON blob for tool_calls turns.
#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: JsonValue,
    /// Only present for "tool" role messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    /// Only present for assistant turns that triggered tool calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    delta: ChatDelta,
    finish_reason: Option<String>,
}

/// Delta content for a single streaming chunk.
/// `tool_calls` is populated when the model wants to call a function.
#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    /// Populated by llama-server when --reasoning-format is set.
    reasoning_content: Option<String>,
    /// Populated when the model decides to call one or more tools.
    tool_calls: Option<Vec<ToolCallDelta>>,
}

/// A single streaming tool-call fragment (index-addressed delta).
#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: Option<ToolCallFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ToolCallFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

/// Accumulator for a single tool call built up from streaming deltas.
#[derive(Default)]
struct StreamingToolCall {
    id: String,
    kind: String,
    name: String,
    arguments: String,
}

/// Result returned by `chat_stream_with_tools_detection`.
/// The visible content has already been sent to `token_tx`; the caller only
/// needs to inspect `tool_calls_raw` to decide whether to loop.
pub struct StreamWithToolsResult {
    /// JSON array of assembled tool calls (empty array when none).
    /// Each element matches the OpenAI `tool_calls` object shape so the
    /// executor can deserialize it with its existing `ToolCall` struct.
    pub tool_calls_raw: JsonValue,
    /// The full visible content text (for history reconstruction).
    pub content: String,
    pub finish_reason: String,
}

#[derive(Clone)]
pub struct RemoteEngine {
    client: Client,
    server_url: String,
    api_key: Option<String>,
    model_name: String,
    enable_thinking: bool,
}

impl RemoteEngine {
    pub fn new(
        server_url: String,
        api_key: Option<String>,
        model_name: Option<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            server_url,
            api_key,
            model_name: model_name.unwrap_or_else(|| "local-model".to_string()),
            enable_thinking: true,
        }
    }

    pub fn with_thinking(mut self, enable: bool) -> Self {
        self.enable_thinking = enable;
        self
    }

    /// Convert `(role, content)` history pairs into `ChatMessage` structs.
    /// Handles special "tool::<id>" roles produced by the agentic executor.
    fn build_messages(messages: Vec<(String, String)>) -> Vec<ChatMessage> {
        messages
            .into_iter()
            .map(|(role, content)| {
                if let Some(tool_id) = role.strip_prefix("tool::") {
                    // Tool result message
                    ChatMessage {
                        role: "tool".to_string(),
                        content: JsonValue::String(content),
                        tool_call_id: Some(tool_id.to_string()),
                        tool_calls: None,
                    }
                } else if role == "assistant" {
                    // The executor stores tool-call turns as a serialised JSON object.
                    // Detect that case and reconstruct the message correctly so the API
                    // receives { role, content, tool_calls } instead of
                    // { role, content: <whole object> } which would cause a 400.
                    match serde_json::from_str::<JsonValue>(&content) {
                        Ok(obj) if obj.is_object() && obj.get("tool_calls").is_some() => {
                            ChatMessage {
                                role,
                                content: obj.get("content").cloned().unwrap_or(JsonValue::Null),
                                tool_call_id: None,
                                tool_calls: obj.get("tool_calls").cloned(),
                            }
                        }
                        Ok(val) => ChatMessage {
                            role,
                            content: val,
                            tool_call_id: None,
                            tool_calls: None,
                        },
                        Err(_) => ChatMessage {
                            role,
                            content: JsonValue::String(content),
                            tool_call_id: None,
                            tool_calls: None,
                        },
                    }
                } else if role == "user" {
                    // Detect multimodal marker written by chat.rs when image
                    // attachments are present.  The marker looks like:
                    //   { "__mm": true, "parts": [ {type, text|image_url}, ... ] }
                    // When found we pass `parts` directly as the content array,
                    // which is the format expected by OpenAI VLM endpoints.
                    match serde_json::from_str::<JsonValue>(&content) {
                        Ok(obj)
                            if obj
                                .get("__mm")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false) =>
                        {
                            ChatMessage {
                                role,
                                content: obj
                                    .get("parts")
                                    .cloned()
                                    .unwrap_or(JsonValue::String(content)),
                                tool_call_id: None,
                                tool_calls: None,
                            }
                        }
                        _ => ChatMessage {
                            role,
                            content: JsonValue::String(content),
                            tool_call_id: None,
                            tool_calls: None,
                        },
                    }
                } else {
                    ChatMessage {
                        role,
                        content: JsonValue::String(content),
                        tool_call_id: None,
                        tool_calls: None,
                    }
                }
            })
            .collect()
    }

    fn build_template_kwargs(config: &InferenceConfig) -> serde_json::Map<String, JsonValue> {
        let mut m = serde_json::Map::new();
        m.insert("enable_thinking".into(), JsonValue::Bool(config.enable_thinking));
        if config.enable_thinking && config.thinking_budget_tokens > 0 {
            m.insert(
                "thinking_budget".into(),
                JsonValue::Number(config.thinking_budget_tokens.into()),
            );
        }
        m
    }

    /// Total token limit to send to the server.
    fn effective_max_tokens(config: &InferenceConfig) -> u32 {
        if config.enable_thinking && config.thinking_budget_tokens > 0 {
            config.thinking_budget_tokens + config.max_tokens
        } else {
            config.max_tokens
        }
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<(String, String)>,
        config: &InferenceConfig,
        tx: mpsc::Sender<String>,
    ) -> Result<()> {
        let chat_messages = Self::build_messages(messages);
        let chat_template_kwargs = Self::build_template_kwargs(config);

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: chat_messages,
            stream: true,
            temperature: config.temperature,
            top_p: config.top_p,
            max_tokens: Self::effective_max_tokens(config),
            cache_prompt: Some(true),
            chat_template_kwargs: Some(chat_template_kwargs),
            tools: None,
            tool_choice: None,
        };

        let url = format!("{}/v1/chat/completions", self.server_url.trim_end_matches('/'));
        let mut req = self.client.post(&url).json(&request);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.context("Failed to connect to LLM server")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM server error {}: {}", status, body);
        }

        let mut stream = response.bytes_stream();

        // State machine for parsing inline <think>…</think> when reasoning_format
        // is not set but the model still emits thinking tokens inside content.
        let mut in_think_tag = false;
        let mut tag_buf = String::new();

        // Line buffer: accumulates bytes across HTTP chunks so that a `data: …`
        // line split across two chunks is never dropped.
        let mut line_buf = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("Failed to read response stream")?;
            line_buf.push_str(&String::from_utf8_lossy(&bytes));

            // Process every complete line (terminated by \n) in the buffer.
            while let Some(nl) = line_buf.find('\n') {
                let line = line_buf[..nl].trim_end_matches('\r').to_string();
                line_buf = line_buf[nl + 1..].to_string();

                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    if !tag_buf.is_empty() {
                        let _ = tx.send(tag_buf.clone()).await;
                    }
                    let _ = tx.send("[DONE]".to_string()).await;
                    return Ok(());
                }

                let Ok(chunk) = serde_json::from_str::<ChatChunk>(data) else {
                    continue;
                };

                let Some(choice) = chunk.choices.first() else {
                    continue;
                };

                // ── reasoning_content field (llama-server --reasoning-format) ──
                if let Some(rc) = &choice.delta.reasoning_content {
                    if !rc.is_empty() && config.enable_thinking {
                        let _ = tx
                            .send(format!("{}{}", THINKING_PREFIX, rc))
                            .await;
                    }
                }

                // ── content field (may contain inline <think> tags) ────────────
                if let Some(content) = &choice.delta.content {
                    if content.is_empty() {
                        if choice.finish_reason.is_some() {
                            if !tag_buf.is_empty() {
                                let _ = tx.send(tag_buf.clone()).await;
                            }
                            let _ = tx.send("[DONE]".to_string()).await;
                            return Ok(());
                        }
                        continue;
                    }

                    self.process_content(content, &mut in_think_tag, &mut tag_buf, &tx, config)
                        .await;
                }

                if choice.finish_reason.is_some() {
                    if !tag_buf.is_empty() {
                        let _ = tx.send(tag_buf.clone()).await;
                        tag_buf.clear();
                    }
                    let _ = tx.send("[DONE]".to_string()).await;
                    return Ok(());
                }
            }
        }

        // Process any partial line remaining in the buffer after the stream ends.
        if !line_buf.trim().is_empty() {
            if let Some(data) = line_buf.trim().strip_prefix("data: ") {
                if data == "[DONE]" {
                    // fall through
                } else if let Ok(chunk) = serde_json::from_str::<ChatChunk>(data) {
                    if let Some(choice) = chunk.choices.first() {
                        if let Some(content) = &choice.delta.content {
                            if !content.is_empty() {
                                self.process_content(
                                    content,
                                    &mut in_think_tag,
                                    &mut tag_buf,
                                    &tx,
                                    config,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        }

        if !tag_buf.is_empty() {
            let _ = tx.send(tag_buf.clone()).await;
        }
        let _ = tx.send("[DONE]".to_string()).await;
        Ok(())
    }

    /// Streaming chat completion that also detects tool calls.
    ///
    /// This replaces the old non-streaming `chat_complete_with_tools` in the
    /// agentic executor loop.  Visible content and reasoning tokens are piped
    /// to `token_tx` in real-time so the user sees output immediately.
    /// When the model requests tool calls the assembled call objects are
    /// returned in `StreamWithToolsResult.tool_calls_raw` for the executor to
    /// dispatch; the caller does NOT need to send `[DONE]` itself when tool
    /// calls are present (the loop will continue), but MUST send it after the
    /// final tool-free turn.
    pub async fn chat_stream_with_tools_detection(
        &self,
        messages: &[(String, String)],
        config: &InferenceConfig,
        tools: &[ToolDefinition],
        token_tx: &mpsc::Sender<String>,
    ) -> Result<StreamWithToolsResult> {
        let chat_messages = Self::build_messages(messages.to_vec());
        let chat_template_kwargs = Self::build_template_kwargs(config);

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: chat_messages,
            stream: true,
            temperature: config.temperature,
            top_p: config.top_p,
            max_tokens: Self::effective_max_tokens(config),
            cache_prompt: Some(true),
            chat_template_kwargs: Some(chat_template_kwargs),
            tools: Some(tools.to_vec()),
            tool_choice: Some(JsonValue::String("auto".to_string())),
        };

        let url = format!("{}/v1/chat/completions", self.server_url.trim_end_matches('/'));
        let mut req = self.client.post(&url).json(&request);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.context("Failed to connect to LLM server")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM server error {}: {}", status, body);
        }

        let mut stream = response.bytes_stream();

        // Accumulators
        let mut tool_call_slots: Vec<StreamingToolCall> = Vec::new();
        let mut visible_content = String::new();
        let mut finish_reason = "stop".to_string();

        // State machine for inline <think> tag handling (same as chat_stream)
        let mut in_think_tag = false;
        let mut tag_buf = String::new();

        // Cross-chunk line buffer
        let mut line_buf = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("Failed to read response stream")?;
            line_buf.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(nl) = line_buf.find('\n') {
                let line = line_buf[..nl].trim_end_matches('\r').to_string();
                line_buf = line_buf[nl + 1..].to_string();

                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    if !tag_buf.is_empty() {
                        let _ = token_tx.send(tag_buf.clone()).await;
                        tag_buf.clear();
                    }
                    return Ok(StreamWithToolsResult {
                        tool_calls_raw: assemble_tool_calls_json(tool_call_slots),
                        content: visible_content,
                        finish_reason,
                    });
                }

                let Ok(chunk) = serde_json::from_str::<ChatChunk>(data) else {
                    continue;
                };

                let Some(choice) = chunk.choices.first() else {
                    continue;
                };

                // Track the finish reason
                if let Some(fr) = &choice.finish_reason {
                    finish_reason = fr.clone();
                }

                // ── reasoning_content ────────────────────────────────────────
                // Track whether this SSE chunk carries thinking tokens so we can
                // gate tool-call accumulation below.
                let has_reasoning_this_chunk = choice
                    .delta
                    .reasoning_content
                    .as_deref()
                    .map(|rc| !rc.is_empty())
                    .unwrap_or(false);

                if has_reasoning_this_chunk && config.enable_thinking {
                    let rc = choice.delta.reasoning_content.as_deref().unwrap_or("");
                    let _ = token_tx
                        .send(format!("{}{}", THINKING_PREFIX, rc))
                        .await;
                }

                // ── visible content (processed BEFORE tool_calls so that
                //    in_think_tag is up-to-date when we decide whether to accept
                //    structured tool calls) ──────────────────────────────────────
                if let Some(content) = &choice.delta.content {
                    if !content.is_empty() {
                        visible_content.push_str(content);
                        self.process_content(
                            content,
                            &mut in_think_tag,
                            &mut tag_buf,
                            token_tx,
                            config,
                        )
                        .await;
                    }
                }

                // ── tool_calls deltas ────────────────────────────────────────
                // Skip tool-call accumulation when the model is still in its
                // thinking phase.  Two signals indicate a planning-only call:
                //   1. This very chunk also carries reasoning_content tokens
                //      (the model is writing the call as part of its thinking).
                //   2. We are inside an inline <think>…</think> block in the
                //      content stream.
                // In both cases the delta.tool_calls is the model narrating its
                // plan, not an actual dispatch request.
                let in_reasoning_phase = has_reasoning_this_chunk || in_think_tag;
                if !in_reasoning_phase {
                    if let Some(tc_deltas) = &choice.delta.tool_calls {
                        for tc in tc_deltas {
                            while tool_call_slots.len() <= tc.index {
                                tool_call_slots.push(StreamingToolCall::default());
                            }
                            let slot = &mut tool_call_slots[tc.index];
                            if let Some(id) = &tc.id {
                                slot.id.push_str(id);
                            }
                            if let Some(kind) = &tc.kind {
                                slot.kind.push_str(kind);
                            }
                            if let Some(func) = &tc.function {
                                if let Some(name) = &func.name {
                                    slot.name.push_str(name);
                                }
                                if let Some(args) = &func.arguments {
                                    slot.arguments.push_str(args);
                                }
                            }
                        }
                    }
                }

                // When finish_reason is set we can return without waiting for [DONE]
                if choice.finish_reason.is_some() {
                    if !tag_buf.is_empty() {
                        let _ = token_tx.send(tag_buf.clone()).await;
                        tag_buf.clear();
                    }
                    return Ok(StreamWithToolsResult {
                        tool_calls_raw: assemble_tool_calls_json(tool_call_slots),
                        content: visible_content,
                        finish_reason,
                    });
                }
            }
        }

        // Handle any partial line remaining after stream ends
        if !line_buf.trim().is_empty() {
            if let Some(data) = line_buf.trim().strip_prefix("data: ") {
                if let Ok(chunk) = serde_json::from_str::<ChatChunk>(data) {
                    if let Some(choice) = chunk.choices.first() {
                        if let Some(content) = &choice.delta.content {
                            if !content.is_empty() {
                                visible_content.push_str(content);
                                self.process_content(
                                    content,
                                    &mut in_think_tag,
                                    &mut tag_buf,
                                    token_tx,
                                    config,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        }

        if !tag_buf.is_empty() {
            let _ = token_tx.send(tag_buf.clone()).await;
        }

        Ok(StreamWithToolsResult {
            tool_calls_raw: assemble_tool_calls_json(tool_call_slots),
            content: visible_content,
            finish_reason,
        })
    }

    /// Process a content fragment, routing text between `<think>` and `</think>`
    /// tags as thinking tokens (prefixed with THINKING_PREFIX) and everything
    /// else as regular visible tokens.
    async fn process_content(
        &self,
        content: &str,
        in_think: &mut bool,
        tag_buf: &mut String,
        tx: &mpsc::Sender<String>,
        config: &InferenceConfig,
    ) {
        // Fast path: no angle brackets and not inside a tag
        if !*in_think && !content.contains('<') && tag_buf.is_empty() {
            let _ = tx.send(content.to_string()).await;
            return;
        }

        let full = format!("{}{}", tag_buf, content);
        tag_buf.clear();

        let mut cursor = full.as_str();

        while !cursor.is_empty() {
            if *in_think {
                // Looking for </think>
                if let Some(end) = cursor.find("</think>") {
                    let thinking = &cursor[..end];
                    if !thinking.is_empty() && config.enable_thinking {
                        let _ = tx
                            .send(format!("{}{}", THINKING_PREFIX, thinking))
                            .await;
                    }
                    cursor = &cursor[end + "</think>".len()..];
                    *in_think = false;
                } else {
                    // Partial </think> at the end? Buffer it.
                    let keep = partial_tag_suffix(cursor, "</think>");
                    let emit = &cursor[..cursor.len() - keep];
                    if !emit.is_empty() && config.enable_thinking {
                        let _ = tx
                            .send(format!("{}{}", THINKING_PREFIX, emit))
                            .await;
                    }
                    if keep > 0 {
                        tag_buf.push_str(&cursor[cursor.len() - keep..]);
                    }
                    break;
                }
            } else {
                // Looking for <think>
                if let Some(start) = cursor.find("<think>") {
                    let before = &cursor[..start];
                    if !before.is_empty() {
                        let _ = tx.send(before.to_string()).await;
                    }
                    cursor = &cursor[start + "<think>".len()..];
                    *in_think = true;
                } else {
                    // Partial <think> at the end?
                    let keep = partial_tag_suffix(cursor, "<think>");
                    let emit = &cursor[..cursor.len() - keep];
                    if !emit.is_empty() {
                        let _ = tx.send(emit.to_string()).await;
                    }
                    if keep > 0 {
                        tag_buf.push_str(&cursor[cursor.len() - keep..]);
                    }
                    break;
                }
            }
        }
    }

    /// Non-streaming chat completion — kept for any callers that still need it
    /// (e.g. connection testing).  The agentic executor loop now uses
    /// `chat_stream_with_tools_detection` instead.
    pub async fn chat_complete_with_tools(
        &self,
        messages: &[(String, String)],
        config: &InferenceConfig,
        tools: &[ToolDefinition],
    ) -> Result<JsonValue> {
        let chat_messages = Self::build_messages(messages.to_vec());
        let chat_template_kwargs = Self::build_template_kwargs(config);

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: chat_messages,
            stream: false,
            temperature: config.temperature,
            top_p: config.top_p,
            max_tokens: Self::effective_max_tokens(config),
            cache_prompt: Some(true),
            chat_template_kwargs: Some(chat_template_kwargs),
            tools: Some(tools.to_vec()),
            tool_choice: Some(JsonValue::String("auto".to_string())),
        };

        let url = format!("{}/v1/chat/completions", self.server_url.trim_end_matches('/'));
        let mut req = self.client.post(&url).json(&request);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.context("Failed to connect to LLM server")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM server error {}: {}", status, body);
        }
        let json: JsonValue = response.json().await?;
        Ok(json)
    }

    pub async fn test_connection(&self) -> Result<bool> {
        let url = format!("{}/v1/models", self.server_url.trim_end_matches('/'));
        let mut req = self.client.get(&url);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        Ok(req.send().await?.status().is_success())
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/v1/models", self.server_url.trim_end_matches('/'));
        let mut req = self.client.get(&url);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let body: serde_json::Value = req.send().await?.json().await?;
        Ok(body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Assemble accumulated `StreamingToolCall` slots into the standard OpenAI
/// tool_calls JSON array shape that the executor's `ToolCall` struct expects.
fn assemble_tool_calls_json(slots: Vec<StreamingToolCall>) -> JsonValue {
    let calls: Vec<JsonValue> = slots
        .into_iter()
        .filter(|s| !s.name.is_empty())
        .map(|s| {
            serde_json::json!({
                "id": if s.id.is_empty() { uuid::Uuid::new_v4().to_string() } else { s.id },
                "type": if s.kind.is_empty() { "function".to_string() } else { s.kind },
                "function": {
                    "name": s.name,
                    "arguments": s.arguments,
                }
            })
        })
        .collect();
    JsonValue::Array(calls)
}

/// Returns how many trailing bytes of `s` could be the start of `tag`.
fn partial_tag_suffix(s: &str, tag: &str) -> usize {
    let bytes = s.as_bytes();
    let tag_bytes = tag.as_bytes();
    for len in (1..tag_bytes.len().min(bytes.len() + 1)).rev() {
        if bytes.ends_with(&tag_bytes[..len]) {
            return len;
        }
    }
    0
}

use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::time::Duration;
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
    /// llama.cpp --jinja: JSON-schema-constrained sampling. When populated the
    /// server will mask logits at decode time so the output is guaranteed to
    /// match the schema. For tool calls we build a `oneOf` union of every
    /// allowed (tool_name, arguments_schema) pair — see
    /// `build_tool_call_response_format`.
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<JsonValue>,
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
        // No global timeout — streaming responses can take arbitrarily long.
        // We set only a *connection* timeout so a dead server is detected quickly,
        // and a *pool idle* timeout to avoid reusing stale connections after
        // the LLM server restarts.
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_default();
        Self {
            client,
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
    ///
    /// Jinja chat templates (e.g. llama-server) require that all `system`
    /// messages appear at the very beginning of the conversation.  When the
    /// user adds or changes a system prompt mid-session the messages array
    /// can violate this rule and the server returns a 500.  We normalise the
    /// list here: all system messages are collected, their contents are merged
    /// (newline-separated) into a single entry, and that entry is hoisted to
    /// position 0 before the rest of the history.
    fn build_messages(messages: Vec<(String, String)>) -> Vec<ChatMessage> {
        // Hoist system messages to the front.
        let mut system_parts: Vec<String> = Vec::new();
        let mut non_system: Vec<(String, String)> = Vec::new();
        for (role, content) in messages {
            if role == "system" {
                system_parts.push(content);
            } else {
                non_system.push((role, content));
            }
        }

        let mut ordered: Vec<(String, String)> = Vec::new();
        if !system_parts.is_empty() {
            ordered.push(("system".to_string(), system_parts.join("\n\n")));
        }
        ordered.extend(non_system);

        ordered
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
                            // When tool_calls is present the content field may be
                            // null (written by older executor code) or missing.
                            // The OpenAI API spec allows an empty string here but
                            // rejects null with a 400, so we normalise to "".
                            let content_val = match obj.get("content") {
                                Some(JsonValue::Null) | None => {
                                    JsonValue::String(String::new())
                                }
                                Some(v) => v.clone(),
                            };
                            ChatMessage {
                                role,
                                content: content_val,
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
            response_format: None,
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
        tool_choice_override: Option<JsonValue>,
        token_tx: &mpsc::Sender<String>,
        // Optional side-channel that gets notified `(tool_call_id, function_name)`
        // the moment the engine has enough delta data to identify a tool call.
        // Used by the executor to publish an early "pending" event to the UI
        // so the user sees a tool-call card while the model is still streaming
        // arguments, instead of waiting for dispatch.
        tool_call_started_tx: Option<&mpsc::Sender<(String, String)>>,
    ) -> Result<StreamWithToolsResult> {
        let chat_messages = Self::build_messages(messages.to_vec());
        let chat_template_kwargs = Self::build_template_kwargs(config);

        // NOTE: we deliberately DO NOT set `response_format` here.
        //
        // With `--jinja --tools all`, llama.cpp builds its own grammar from the
        // chat template's tool-call wrapping (e.g. Qwen wraps calls in
        // `<tool_call>…</tool_call>`, Hermes uses a different pattern). Passing
        // our own `response_format: json_schema` overrides that grammar with a
        // plain-JSON schema, so the model's output is valid JSON but bypasses
        // the template's tool-call parser — it then leaks out as assistant
        // `content` (or into `reasoning_content` when wrapped in `<think>`)
        // and the executor sees zero `tool_calls`. See the retry / validate
        // flow in skills/executor.rs: a `tool_choice` pin + corrective user
        // message is enough to force the exact tool + valid args on retry,
        // without clobbering the native tool-call grammar.
        let tool_choice = tool_choice_override.unwrap_or_else(|| JsonValue::String("auto".into()));

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
            tool_choice: Some(tool_choice),
            response_format: None,
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
        // Parallel to `tool_call_slots`: whether we've already emitted the
        // "started" notice for that index. Prevents firing the event on every
        // arguments-delta chunk.
        let mut tool_call_announced: Vec<bool> = Vec::new();
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
                                tool_call_announced.push(false);
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

                            // ── Early UI notification ─────────────────────
                            //
                            // As soon as we have the pair (id, name) for this
                            // slot, tell the executor so it can publish a
                            // "pending" tool-call card to the frontend. This
                            // happens during streaming, well before args are
                            // finished, so the user gets instant feedback
                            // that a tool is being prepared. The notice only
                            // fires once per slot — subsequent argument
                            // chunks don't trigger it again.
                            if !tool_call_announced[tc.index]
                                && !slot.id.is_empty()
                                && !slot.name.is_empty()
                            {
                                if let Some(tx) = tool_call_started_tx {
                                    // Best-effort send; if the receiver is
                                    // gone, we don't want to tear down the
                                    // stream for a UI nicety.
                                    let _ = tx
                                        .send((slot.id.clone(), slot.name.clone()))
                                        .await;
                                }
                                tool_call_announced[tc.index] = true;
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

        // See the long comment in `chat_stream_with_tools_detection` about why
        // `response_format` is left as `None` when tools are enabled.
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
            response_format: None,
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

// ── Tool-call schema builder ──────────────────────────────────────────────────
//
// llama.cpp's JSON-schema-constrained sampler silently falls back to
// unconstrained output when the schema contains `$ref`/`$defs` (issue #21228).
// Pydantic-v2-generated schemas ALWAYS carry refs, so every MCP tool's
// input_schema must be flattened inline before being sent to the server.

/// Recursively expand every `$ref` in `schema` against the root's `$defs`
/// (or legacy `definitions`). Removes `$defs`/`definitions` keys from the
/// result so the final document is self-contained.
///
/// Cycles are guarded by a visited set; on a cycle the offending `$ref` is
/// replaced with an empty schema `{}` (accept-anything) rather than recursing
/// forever.
#[allow(dead_code)]
pub fn inline_refs(schema: &JsonValue) -> JsonValue {
    let defs = extract_defs(schema);
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    strip_defs_keys(&mut inline_refs_inner(schema, &defs, &mut visited))
}

#[allow(dead_code)]
fn extract_defs(schema: &JsonValue) -> serde_json::Map<String, JsonValue> {
    let mut defs = serde_json::Map::new();
    if let Some(obj) = schema.as_object() {
        if let Some(d) = obj.get("$defs").and_then(|v| v.as_object()) {
            for (k, v) in d {
                defs.insert(k.clone(), v.clone());
            }
        }
        if let Some(d) = obj.get("definitions").and_then(|v| v.as_object()) {
            for (k, v) in d {
                defs.insert(k.clone(), v.clone());
            }
        }
    }
    defs
}

#[allow(dead_code)]
fn inline_refs_inner(
    node: &JsonValue,
    defs: &serde_json::Map<String, JsonValue>,
    visited: &mut std::collections::HashSet<String>,
) -> JsonValue {
    match node {
        JsonValue::Object(obj) => {
            if let Some(JsonValue::String(r)) = obj.get("$ref") {
                // Only support local refs like "#/$defs/Foo" or "#/definitions/Foo".
                let key = r
                    .trim_start_matches("#/$defs/")
                    .trim_start_matches("#/definitions/");
                if visited.contains(key) {
                    return JsonValue::Object(serde_json::Map::new());
                }
                if let Some(target) = defs.get(key) {
                    visited.insert(key.to_string());
                    let resolved = inline_refs_inner(target, defs, visited);
                    visited.remove(key);
                    return resolved;
                }
                // Unresolvable ref → fall through to empty schema.
                return JsonValue::Object(serde_json::Map::new());
            }
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (k, v) in obj {
                if k == "$defs" || k == "definitions" {
                    continue;
                }
                out.insert(k.clone(), inline_refs_inner(v, defs, visited));
            }
            JsonValue::Object(out)
        }
        JsonValue::Array(arr) => {
            JsonValue::Array(arr.iter().map(|v| inline_refs_inner(v, defs, visited)).collect())
        }
        other => other.clone(),
    }
}

#[allow(dead_code)]
fn strip_defs_keys(node: &mut JsonValue) -> JsonValue {
    if let Some(obj) = node.as_object_mut() {
        obj.remove("$defs");
        obj.remove("definitions");
    }
    node.clone()
}

/// Build the OpenAI-flavoured `response_format: {"type":"json_schema", ...}`
/// payload that constrains the model's next output to a single valid tool call.
///
/// **Not currently wired into requests.** Setting this alongside `tools` +
/// `--jinja` overrides llama.cpp's template-driven tool-call grammar (which
/// wraps calls in template-specific markers such as `<tool_call>…</tool_call>`
/// for Qwen, or Hermes' bespoke format). The server then returns the raw JSON
/// we ask for, but the parser never recognises it as a `tool_calls` payload —
/// it falls out as assistant `content` instead. Kept here for the case where
/// we can one day generate a schema that *matches* the template's exact
/// wrapping, at which point we can wire it back into `ChatRequest`.
///
/// The schema is a two-level wrapper:
/// * Top-level: `{"tool_calls": [ <one-of union> ]}` — matches the OpenAI
///   streaming assistant message shape that llama-server emits with `--jinja`.
/// * Inner `oneOf`: one branch per allowed tool, each branch pinning
///   `function.name` to a constant and embedding the tool's input_schema as
///   `function.arguments`.
///
/// When `pinned_name` is `Some`, only that tool's branch is included — used
/// by the retry path to force the model to re-emit a specific call with
/// valid args.
///
/// Returns `None` when `tools` is empty (nothing to constrain).
#[allow(dead_code)]
pub fn build_tool_call_response_format(
    tools: &[ToolDefinition],
    pinned_name: Option<&str>,
) -> Option<JsonValue> {
    if tools.is_empty() {
        return None;
    }

    let branches: Vec<JsonValue> = tools
        .iter()
        .filter(|t| pinned_name.map_or(true, |name| t.function.name == name))
        .map(|t| {
            let args_schema = inline_refs(&t.function.parameters);
            // Fall back to `{"type":"object"}` when the tool declares no schema
            // so llama-server still accepts the oneOf branch.
            let args_schema = if args_schema.is_object() {
                args_schema
            } else {
                serde_json::json!({"type": "object"})
            };
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "type": {"type": "string", "const": "function"},
                    "function": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "const": t.function.name},
                            "arguments": args_schema,
                        },
                        "required": ["name", "arguments"],
                    }
                },
                "required": ["id", "type", "function"],
            })
        })
        .collect();

    if branches.is_empty() {
        return None;
    }

    let one_of = JsonValue::Array(branches);

    Some(serde_json::json!({
        "type": "json_schema",
        "json_schema": {
            "name": "tool_call",
            "strict": true,
            "schema": {
                "type": "object",
                "properties": {
                    "tool_calls": {
                        "type": "array",
                        "minItems": 1,
                        "items": { "oneOf": one_of }
                    }
                },
                "required": ["tool_calls"],
            }
        }
    }))
}

/// Extract the `function.name` field from an OpenAI-style `tool_choice`
/// object, returning `None` for `"auto"`, `"none"`, or malformed input.
#[allow(dead_code)]
fn extract_pinned_tool_name(choice: &JsonValue) -> Option<String> {
    choice
        .get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

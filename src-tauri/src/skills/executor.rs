/// Agentic tool-calling loop.
///
/// Algorithm:
///   1. Build the chat request with tool definitions attached.
///   2. Send to llama-server using streaming tool detection so the user sees
///      output immediately while tool calls are detected from the stream.
///   3. If the stream ends with tool_calls, dispatch each one via SkillsManager,
///      append the results as `tool` role messages, go to step 2.
///   4. When no tool_calls are returned the final answer was already streamed;
///      just send `[DONE]` on the channel.
///
/// All intermediate tool-call/result steps are emitted as Tauri events so the
/// frontend can show them as expandable cards in the chat.

use anyhow::{Context, Result};
use rusqlite::params as sql_params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use super::manager::SkillsManager;
use crate::db::AppDb;
use crate::engine::remote::RemoteEngine;
use crate::models::InferenceConfig;

const MAX_TOOL_TURNS: usize = 8;
const CODE_RUNNER_SERVER_ID: &str = "code_runner";
/// Maximum number of times we'll re-issue a turn because the model emitted
/// invalid tool-call JSON (missing required fields or unparseable arguments).
/// Beyond this the executor surfaces the validation error instead of looping.
const MAX_TOOL_CALL_RETRIES: usize = 2;
/// Maximum number of times we'll re-issue a turn because the dispatched tool
/// returned a runtime error (e.g. `create_math_document` reported a LaTeX
/// compilation failure). Capped to prevent the same bad payload from burning
/// through `MAX_TOOL_TURNS` when the model can't self-correct.
const MAX_TOOL_ERROR_RETRIES: usize = 2;
/// Fraction of the server's n_ctx that tools + history are allowed to occupy
/// before the tool list gets pruned. Leaves the remaining ~15% for the model's
/// actual response.
const CTX_BUDGET_FRACTION: f32 = 0.85;

// ── OpenAI tool schema types (for request serialisation) ──────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String, // always "function"
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value, // JSON Schema object
}

// ── OpenAI tool_calls response types ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

// ── SkillsExecutor ────────────────────────────────────────────────────────

pub struct SkillsExecutor {
    pub skills: Arc<SkillsManager>,
    /// When set, the code_runner built-in tools are injected and dispatched natively.
    code_runner_db: Option<Arc<Mutex<AppDb>>>,
    code_runner_conv_id: Option<String>,
    /// Counts hard code-execution failures (exit_code != 0 or non-empty stderr) for retry capping.
    code_exec_retries: AtomicUsize,
    /// Counts tool-call JSON validation failures (missing required fields,
    /// unparseable arguments). Capped by `MAX_TOOL_CALL_RETRIES`.
    tool_call_retries: AtomicUsize,
    /// Counts successive runtime errors returned by dispatched tools (e.g.
    /// LaTeX compile failures) so we can force a bounded retry with a pinned
    /// `tool_choice`. Capped by `MAX_TOOL_ERROR_RETRIES`.
    tool_error_retries: AtomicUsize,
}

impl SkillsExecutor {
    pub fn new(skills: Arc<SkillsManager>) -> Self {
        Self {
            skills,
            code_runner_db: None,
            code_runner_conv_id: None,
            code_exec_retries: AtomicUsize::new(0),
            tool_call_retries: AtomicUsize::new(0),
            tool_error_retries: AtomicUsize::new(0),
        }
    }

    /// Enable the built-in code execution tools for this invocation.
    pub fn with_code_runner(
        mut self,
        db: Arc<Mutex<AppDb>>,
        conversation_id: String,
    ) -> Self {
        self.code_runner_db = Some(db);
        self.code_runner_conv_id = Some(conversation_id);
        self
    }

    /// Build the *full* set of OpenAI-compatible tool definitions from every
    /// connected MCP server, plus the built-in `code_runner` tools when the
    /// runner is enabled.
    ///
    /// This is the unscoped superset; the agentic loop then runs it through
    /// [`Self::scope_tools_for_turn`] to shrink the list to what's actually
    /// relevant to the current user turn.
    pub async fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = self
            .skills
            .all_tools()
            .await
            .into_iter()
            .map(|tagged| ToolDefinition {
                kind: "function".to_string(),
                function: FunctionDef {
                    name: format!("{}__{}", tagged.server_id, tagged.tool.name),
                    description: tagged.tool.description.unwrap_or_default(),
                    parameters: tagged.tool.input_schema,
                },
            })
            .collect();

        if self.code_runner_db.is_some() {
            tools.push(code_runner_execute_tool());
            tools.push(code_runner_list_artifacts_tool());
        }

        tools
    }

    /// Scope the full tool list down to what's relevant for the *current* turn.
    ///
    /// Algorithm:
    /// 1. Score each `server_id` (prefix of the qualified name before `__`) by
    ///    keyword-overlap with the last two user messages.
    /// 2. Honour "sticky tools": the server that fulfilled the most recent
    ///    assistant tool call is always kept, so follow-up turns don't drop the
    ///    tool the model just used.
    /// 3. Always keep lightweight utility servers (`file_ops`) regardless of
    ///    intent — EXCEPT `code_runner` is dropped when strong LaTeX intent is
    ///    detected (belt-and-braces against the `subprocess.run(['pdflatex'])`
    ///    anti-pattern that still slips through the system prompt).
    /// 4. If scoring produced zero hits AND there's no sticky server, return
    ///    the full set (general-chat fallback).
    /// 5. Finally run a token-budget check: if the serialised tools + recent
    ///    history would exceed `n_ctx * CTX_BUDGET_FRACTION`, iteratively drop
    ///    the lowest-scored servers until it fits.
    fn scope_tools_for_turn(
        &self,
        all_tools: Vec<ToolDefinition>,
        messages: &[(String, String)],
        n_ctx: usize,
    ) -> Vec<ToolDefinition> {
        let recent_user_text = collect_recent_user_text(messages, 2);
        let sticky_server = last_assistant_tool_server(messages);
        let strong_latex = has_strong_latex_intent(&recent_user_text);

        // Group tools by server_id so we can include/exclude whole servers.
        let mut by_server: std::collections::BTreeMap<String, Vec<ToolDefinition>> =
            std::collections::BTreeMap::new();
        for t in all_tools {
            let sid = extract_server_id(&t.function.name);
            by_server.entry(sid).or_default().push(t);
        }

        // Score every server present in the set.
        let scores: Vec<(String, i32)> = by_server
            .keys()
            .map(|sid| (sid.clone(), score_server(sid, &recent_user_text)))
            .collect();

        let any_hit = scores.iter().any(|(_, s)| *s > 0) || sticky_server.is_some();

        let kept: Vec<(String, Vec<ToolDefinition>, i32)> = by_server
            .into_iter()
            .filter_map(|(sid, tools)| {
                let score = scores
                    .iter()
                    .find(|(s, _)| *s == sid)
                    .map(|(_, s)| *s)
                    .unwrap_or(0);
                let is_sticky = sticky_server.as_deref() == Some(sid.as_str());
                let is_always_on_util = is_always_on(&sid, strong_latex);

                let keep = if !any_hit {
                    true // general chat — keep everything
                } else {
                    score > 0 || is_sticky || is_always_on_util
                };

                if keep {
                    Some((sid, tools, score))
                } else {
                    None
                }
            })
            .collect();

        // Flatten and budget-check.
        budget_trim(kept, messages, n_ctx)
    }

    /// Run the agentic loop.
    ///
    /// Uses `chat_stream_with_tools_detection` so that every turn streams
    /// content tokens to the frontend in real-time.  Tool calls detected in
    /// the stream are dispatched, results appended, and the loop continues.
    pub async fn run(
        &self,
        mut messages: Vec<(String, String)>,
        config: &InferenceConfig,
        engine: &RemoteEngine,
        app: &AppHandle,
        conv_id: &str,
        token_tx: mpsc::Sender<String>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<()> {
        let full_tools = self.build_tool_definitions().await;

        if full_tools.is_empty() {
            // No tools registered — fall back to plain streaming chat.
            return engine.chat_stream(messages, config, token_tx).await;
        }

        // Pull n_ctx from the live llama-server settings so budget-trimming
        // tracks the actual context window. Fall back to 4096 when the state
        // is unreachable (unit tests, detached executor).
        let n_ctx: usize = {
            use tauri::Manager;
            app.try_state::<crate::state::AppState>()
                .and_then(|st| st.settings.lock().ok().map(|s| s.server_context_size as usize))
                .unwrap_or(4096)
        };

        /// A single generated media item collected across all tool-call turns.
        #[derive(Debug)]
        struct GeneratedMedia {
            /// `"image"` or `"video"`
            kind: &'static str,
            /// The live URL returned by the tool (ComfyUI /view endpoint).
            url: String,
            /// Gallery row id injected after saving, if available.
            gallery_id: Option<String>,
            /// Filename hint for the markdown alt-text.
            filename: String,
        }

        // Accumulated across turns so we can inject markdown at the very end
        // if the LLM never included the URL itself.
        let mut generated_media: Vec<GeneratedMedia> = Vec::new();

        // Accumulated HTML from rich_responses tools; injected after the LLM's
        // final response so the LLM never has to deal with large HTML strings.
        let mut generated_html: Vec<String> = Vec::new();

        /// A PDF file created by a pdf_tools tool call.
        #[derive(Debug)]
        struct GeneratedPdf {
            /// Filename used as the artifact title (e.g. "report.pdf").
            title: String,
            /// JSON metadata stored as artifact content: path, filename, pages.
            content: String,
        }

        // Accumulated PDF artifacts; injected as <artifact type="pdf"> tags at
        // the end so chat.rs saves them and MessageBubble renders a card.
        let mut generated_pdfs: Vec<GeneratedPdf> = Vec::new();

        // Helper: emit app_log event from the executor
        let log_event = |level: &str, msg: String| {
            let _ = app.emit("app_log", json!({
                "level": level,
                "message": msg,
                "ts": chrono::Utc::now().to_rfc3339(),
            }));
        };

        // When a tool call failed validation on the previous turn, this carries
        // an OpenAI-shaped `tool_choice` object ({type:"function",function:{name}})
        // that pins the retry to the exact tool the model flubbed. Cleared on
        // every successful turn so we fall back to `auto` on the happy path.
        let mut pinned_tool_choice: Option<Value> = None;

        // When the *previous* turn's tool dispatch returned an error-shaped
        // result (top-level `"error"` field — e.g. a LaTeX compilation failure),
        // this carries `(fn_name, short_error_msg)`. The next iteration uses
        // it to (a) inject a corrective user message if the model tried to
        // walk away from the failure with a text-only response, and
        // (b) re-pin `tool_choice` so the retry hits the same tool again.
        let mut prior_tool_error: Option<(String, String)> = None;

        for turn in 0..MAX_TOOL_TURNS {
            // Check cancellation at the start of every turn (covers the gap
            // between tool dispatch and the next LLM call).
            if cancelled.load(Ordering::Relaxed) {
                log_event("info", format!("[executor] Turn {} — cancelled by user", turn));
                let _ = token_tx.send("[DONE]".to_string()).await;
                return Ok(());
            }

            // ── Per-turn tool scoping ─────────────────────────────────────────
            // Rebuild the tool list every turn because the relevant scope can
            // change as the conversation evolves (user pivots from LaTeX to
            // image generation, etc.) and because the assistant's last tool
            // call updates the sticky-server bias.
            let tools =
                self.scope_tools_for_turn(full_tools.clone(), &messages, n_ctx);

            if tools.is_empty() {
                // Shouldn't happen (scoping always keeps at least something in
                // the general-chat fallback), but be safe.
                log_event(
                    "warn",
                    format!(
                        "[executor] Turn {} — scoped tool list is empty; falling back to plain chat",
                        turn
                    ),
                );
                return engine.chat_stream(messages, config, token_tx).await;
            }

            log_event(
                "info",
                format!(
                    "[executor] Turn {} — sending request to LLM ({} scoped tools{})",
                    turn,
                    tools.len(),
                    if pinned_tool_choice.is_some() {
                        ", pinned by retry"
                    } else {
                        ""
                    }
                ),
            );

            // ── Streaming call: pipes content to token_tx, detects tool calls ─
            // ── Early tool-call notification channel ─────────────────────
            //
            // The engine emits `(tool_call_id, function_name)` as soon as it
            // can identify a tool call in the SSE stream. We forward each
            // notice to the frontend as a `chat_tool_call_pending` event so
            // the UI shows an amber "preparing…" card immediately, rather
            // than waiting for streaming to finish and the validator + full
            // `chat_tool_call` event to fire. On long compiles (LaTeX,
            // image gen, video gen) this is the difference between a blank
            // pause and obvious activity.
            let (tool_start_tx, mut tool_start_rx) =
                tokio::sync::mpsc::channel::<(String, String)>(16);
            let app_for_pending = app.clone();
            let conv_id_for_pending = conv_id.to_string();
            let turn_for_pending = turn as u32;
            let pending_forwarder = tokio::spawn(async move {
                while let Some((tc_id, fn_name)) = tool_start_rx.recv().await {
                    let _ = app_for_pending.emit(
                        "chat_tool_call_pending",
                        json!({
                            "conversation_id": conv_id_for_pending,
                            "tool_call_id": tc_id,
                            "function_name": fn_name,
                            "turn": turn_for_pending,
                        }),
                    );
                    {
                        use tauri::Manager;
                        if let Some(st) =
                            app_for_pending.try_state::<crate::state::AppState>()
                        {
                            let _ = st.event_tx.send(
                                crate::api_server::events::ApiEvent::ChatToolCall {
                                    conversation_id: conv_id_for_pending.clone(),
                                    tool_call_id: tc_id,
                                    function_name: fn_name,
                                    arguments: json!({}),
                                    turn: turn_for_pending,
                                },
                            );
                        }
                    }
                }
            });

            let result = match engine
                .chat_stream_with_tools_detection(
                    &messages,
                    config,
                    &tools,
                    pinned_tool_choice.take(),
                    &token_tx,
                    Some(&tool_start_tx),
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let err_str = e.to_string();
                    // The LLM server returned a 500 because it tried to embed huge/complex
                    // content (e.g. an entire HTML page) inside a tool-call JSON argument,
                    // producing malformed JSON. Recover: inject a corrective message and
                    // retry once in plain-streaming mode (no tools) so the LLM can output
                    // the content directly as an artifact tag.
                    if err_str.contains("Failed to parse tool call arguments")
                        || (err_str.contains("500") && err_str.contains("parse"))
                    {
                        log_event("warn", format!(
                            "[executor] Turn {} — LLM tool-call JSON parse error; recovering without tools. err={}",
                            turn, &err_str[..err_str.len().min(200)]
                        ));
                        messages.push((
                            "user".to_string(),
                            "Your previous response could not be sent because you tried to pass \
                             large content (such as HTML or CSS) as a tool-call argument, \
                             which breaks JSON serialisation. \
                             STOP using execute_code for HTML/CSS/web content entirely. \
                             Output the content DIRECTLY in an artifact tag right now — \
                             for example: <artifact type=\"html\" title=\"Page Title\">...HTML here...</artifact>. \
                             Do NOT call any tool. Write the artifact tag immediately."
                                .to_string(),
                        ));
                        engine.chat_stream(messages, config, token_tx.clone()).await?;
                        let _ = token_tx.send("[DONE]".to_string()).await;
                        return Ok(());
                    }
                    return Err(e);
                }
            };

            // Streaming finished: drop the sender so the forwarder task sees
            // the channel close and exits, then await it so we don't leak.
            drop(tool_start_tx);
            let _ = pending_forwarder.await;

            // Deserialise the assembled tool calls (empty Vec when none)
            let tool_calls: Vec<ToolCall> = serde_json::from_value(result.tool_calls_raw.clone())
                .unwrap_or_default();

            if tool_calls.is_empty() {
                // ── Safety net: model walked away from a failed tool ────────
                //
                // If the previous turn's tool dispatch returned an error
                // (e.g. LaTeX compile failure) and this turn the model just
                // narrates "let me fix that" without actually re-calling the
                // tool, fail upwards to the user instead of silently burying
                // the error. The pinned `tool_choice` on this turn *should*
                // have forced a tool call — when it didn't, the model is
                // stuck and extra rounds won't help.
                if let Some((bad_tool, bad_err)) = prior_tool_error.take() {
                    let attempts = self.tool_error_retries.fetch_add(1, Ordering::Relaxed) + 1;
                    if attempts > MAX_TOOL_ERROR_RETRIES {
                        log_event("warn", format!(
                            "[executor] Turn {} — `{}` failed repeatedly and model refused to \
                             retry via tool call (attempt {}/{}). Surfacing error to user.",
                            turn, bad_tool, attempts, MAX_TOOL_ERROR_RETRIES
                        ));
                        let _ = token_tx
                            .send(format!(
                                "\n\n[Tool `{}` failed repeatedly: {}. The model was unable to \
                                 produce a working retry. Please adjust your request.]",
                                bad_tool, bad_err
                            ))
                            .await;
                        let _ = token_tx.send("[DONE]".to_string()).await;
                        return Ok(());
                    }
                    log_event("warn", format!(
                        "[executor] Turn {} — model emitted no tool_calls after `{}` error \
                         (attempt {}/{}); forcing retry with pinned tool_choice.",
                        turn, bad_tool, attempts, MAX_TOOL_ERROR_RETRIES
                    ));
                    messages.push((
                        "user".to_string(),
                        format!(
                            "Your previous call to `{tool}` failed: {err}. \
                             Do NOT explain or apologise in plain text. Your next response MUST \
                             be a tool call to `{tool}` with corrected arguments that fix the \
                             problem above. Emit the tool call right now — no surrounding prose.",
                            tool = bad_tool,
                            err = bad_err,
                        ),
                    ));
                    pinned_tool_choice = Some(json!({
                        "type": "function",
                        "function": { "name": bad_tool },
                    }));
                    continue;
                }

                // No tool calls — the final answer was already streamed live.
                log_event("info", format!(
                    "[executor] Turn {} — no tool calls detected, streaming complete (finish_reason={})",
                    turn, result.finish_reason
                ));

                // ── Append media markdown if the LLM didn't include it ───
                // The LLM sometimes just describes the result in text without
                // embedding the image/video. Check each generated media item;
                // if its URL does not appear anywhere in the response, inject
                // a markdown image/video block so the user can see it directly.
                if !generated_media.is_empty() {
                    // Use only the visible text that comes after the last </think> tag.
                    // Models with reasoning (Qwen3, DeepSeek-R1, etc.) include the full
                    // tool-result JSON — including the video/image URL — inside their
                    // <think>…</think> block.  A check against the raw result.content
                    // would falsely match the URL in that hidden thinking section and
                    // suppress injection, leaving the chat with no media link.
                    let visible_response: &str = result.content
                        .rfind("</think>")
                        .map(|i| &result.content[i + 8..])
                        .unwrap_or(&result.content);
                    let mut appendix = String::new();
                    for media in &generated_media {
                        // Only skip injection if the URL already appears as a markdown
                        // link "(url)" in the visible (post-thinking) part of the reply.
                        let already_embedded = visible_response
                            .contains(&format!("({})", media.url));
                        if !already_embedded {
                            let stem = media.filename
                                .rsplit('.')
                                .last()
                                .unwrap_or(&media.filename);
                            // Both images and videos use the markdown image syntax.
                            // The frontend's markdown renderer detects video URLs
                            // via filename extension and renders a <video> element
                            // instead of <img> automatically.
                            appendix.push_str(&format!(
                                "\n\n![{}]({})",
                                stem, media.url
                            ));
                        }
                    }
                    if !appendix.is_empty() {
                        log_event("info", format!(
                            "[executor] Appending {} media item(s) to response",
                            generated_media.len()
                        ));
                        let _ = token_tx.send(appendix).await;
                    }
                }

                // ── Inject buffered rich-response HTML ───────────────────
                if !generated_html.is_empty() {
                    log_event("info", format!(
                        "[executor] Injecting {} rich HTML block(s) into response",
                        generated_html.len()
                    ));
                    for html in &generated_html {
                        let _ = token_tx.send(format!("\n\n{}", html)).await;
                    }
                }

                // ── Inject PDF artifact tags ──────────────────────────────
                // Each tag is parsed by chat.rs (saved to DB) and by
                // MessageBubble (rendered as a clickable ArtifactCard).
                if !generated_pdfs.is_empty() {
                    log_event("info", format!(
                        "[executor] Injecting {} PDF artifact tag(s) into response",
                        generated_pdfs.len()
                    ));
                    for pdf in &generated_pdfs {
                        let tag = format!(
                            "\n\n<artifact type=\"pdf\" title=\"{}\">{}</artifact>",
                            pdf.title, pdf.content
                        );
                        let _ = token_tx.send(tag).await;
                    }
                }

                let _ = token_tx.send("[DONE]".to_string()).await;
                return Ok(());
            }

            log_event("info", format!(
                "[executor] Turn {} — {} tool call(s) detected", turn, tool_calls.len()
            ));

            // ── Validate every tool-call's arguments before dispatching any ──
            //
            // llama-server can (and does) stream truncated JSON into
            // `function.arguments` when the model hits its token budget or
            // when grammar enforcement misfires. The old code silently
            // collapsed such failures to `{}`, which then reached MCP tools
            // and triggered cryptic `Field required` Pydantic errors. We now
            // validate each call against its declared `input_schema` and, on
            // failure, inject a corrective user message and loop so the model
            // can retry with valid args. The pin on `tool_choice` guarantees
            // it retries the SAME tool instead of bailing to freeform text.
            let mut validated: Vec<(ToolCall, Value)> = Vec::with_capacity(tool_calls.len());
            let mut validation_error: Option<(String, String)> = None; // (fn_name, err)

            for tc in &tool_calls {
                match validate_tool_args(&tc.function.name, &tc.function.arguments, &tools) {
                    Ok(parsed) => validated.push((tc.clone(), parsed)),
                    Err(err) => {
                        validation_error = Some((tc.function.name.clone(), err));
                        break;
                    }
                }
            }

            if let Some((bad_name, err)) = validation_error {
                let attempts = self.tool_call_retries.fetch_add(1, Ordering::Relaxed) + 1;
                log_event(
                    "warn",
                    format!(
                        "[executor] Turn {} — tool '{}' args invalid (attempt {}/{}): {}",
                        turn, bad_name, attempts, MAX_TOOL_CALL_RETRIES, err
                    ),
                );

                if attempts > MAX_TOOL_CALL_RETRIES {
                    // Exhausted — surface a real error to the user instead of
                    // dispatching the tool with `{}` as we used to.
                    let _ = token_tx
                        .send(format!(
                            "\n\n[Tool call failed: `{}` was repeatedly emitted with invalid \
                             arguments ({}). The model was unable to produce a valid JSON payload \
                             after {} attempts. Please rephrase your request.]",
                            bad_name, err, MAX_TOOL_CALL_RETRIES
                        ))
                        .await;
                    let _ = token_tx.send("[DONE]".to_string()).await;
                    return Ok(());
                }

                // Build the list of required fields for the corrective
                // message so the model has everything it needs to fix its
                // output on the next pass.
                let required_list = required_fields_for(&bad_name, &tools)
                    .map(|v| v.join(", "))
                    .unwrap_or_else(|| "(see tool schema)".to_string());

                let corrective = format!(
                    "Your previous call to `{tool}` had invalid arguments: {err}.\n\
                     You MUST emit a single valid JSON object that includes EVERY required \
                     field: {fields}. An empty `{{}}` is never acceptable. \
                     Call `{tool}` again right now with correctly-filled arguments. \
                     Do NOT write the content inline as assistant text — it belongs inside the \
                     tool-call arguments.",
                    tool = bad_name,
                    err = err,
                    fields = required_list,
                );
                messages.push(("user".to_string(), corrective));

                // Pin the retry to the specific tool so the model can't dodge
                // into a different tool (or plain text) to avoid the hard
                // JSON requirement.
                pinned_tool_choice = Some(json!({
                    "type": "function",
                    "function": { "name": bad_name }
                }));
                continue; // retry this turn
            }

            // All tool calls validated — reset the retry counter for the
            // next independent flake cycle.
            self.tool_call_retries.store(0, Ordering::Relaxed);

            // ── Tool calls detected — reconstruct the assistant history message ─
            //
            // We store the assistant message in the same JSON shape as the old
            // non-streaming path so that `build_messages` in remote.rs can
            // correctly reconstruct it on the next turn.
            let assistant_history_msg = json!({
                "role": "assistant",
                "content": if result.content.is_empty() {
                    Value::Null
                } else {
                    Value::String(result.content.clone())
                },
                "tool_calls": result.tool_calls_raw,
            });
            messages.push((
                "assistant".to_string(),
                serde_json::to_string(&assistant_history_msg).unwrap_or_default(),
            ));

            // Per-turn accumulator for dispatched tools that returned an
            // error-shaped JSON result. At end of the dispatch loop, if this
            // has any entries we retry the turn with `tool_choice` pinned to
            // the first failing tool so the model is forced to re-emit with
            // corrected arguments instead of narrating around the failure.
            let mut turn_tool_errors: Vec<(String, String)> = Vec::new();

            // ── Execute each tool call ─────────────────────────────────────
            for (tc, args) in validated {
                let tc = &tc;
                let fn_name = &tc.function.name;

                let args_preview = serde_json::to_string(&args)
                    .unwrap_or_default()
                    .chars()
                    .take(120)
                    .collect::<String>();
                log_event("info", format!(
                    "[executor] Dispatching tool '{}' (id={}) args={}", fn_name, tc.id, args_preview
                ));

                // Emit tool-call event to frontend and HTTP SSE clients
                let _ = app.emit(
                    "chat_tool_call",
                    json!({
                        "conversation_id": conv_id,
                        "tool_call_id": tc.id,
                        "function_name": fn_name,
                        "arguments": args,
                        "turn": turn,
                    }),
                );
                {
                    use tauri::Manager;
                    if let Some(st) = app.try_state::<crate::state::AppState>() {
                        let _ = st.event_tx.send(crate::api_server::events::ApiEvent::ChatToolCall {
                            conversation_id: conv_id.to_string(),
                            tool_call_id: tc.id.clone(),
                            function_name: fn_name.to_string(),
                            arguments: args.clone(),
                            turn: turn as u32,
                        });
                    }
                }

                // Resolve any local gallery image URLs to temp files so the
                // Python subprocess never needs to make HTTP requests to localhost
                // (which fails on Windows due to IPv4/IPv6 loopback differences).
                let (resolved_args, _temp_files) = {
                    use tauri::Manager;
                    let state = app.state::<crate::state::AppState>();
                    resolve_gallery_image_urls(args, &state)
                };

                // Signal that a tool is running so the idle-watcher does not
                // kill the LLM server while we wait for the external process.
                {
                    use tauri::Manager;
                    let state = app.state::<crate::state::AppState>();
                    state.tool_active.store(true, Ordering::Relaxed);
                }

                let result_text = self
                    .dispatch_tool_call(fn_name, resolved_args, app, conv_id)
                    .await
                    .unwrap_or_else(|e| {
                        let err_msg = format!("Tool '{}' error: {}", fn_name, e);
                        log::error!("{}", err_msg);
                        let _ = app.emit("app_log", json!({
                            "level": "error",
                            "message": err_msg,
                            "ts": chrono::Utc::now().to_rfc3339(),
                        }));
                        json!({"error": e.to_string()}).to_string()
                    });

                // Tool finished — clear the active flag so the idle-watcher
                // resumes normal keep-alive behaviour.
                {
                    use tauri::Manager;
                    let state = app.state::<crate::state::AppState>();
                    state.tool_active.store(false, Ordering::Relaxed);
                }

                // ── Check cancellation after potentially long tool call ──
                if cancelled.load(Ordering::Relaxed) {
                    log_event("info", format!(
                        "[executor] Tool '{}' completed but generation was cancelled — stopping", fn_name
                    ));
                    let _ = token_tx.send("[DONE]".to_string()).await;
                    return Ok(());
                }

                // ── Auto-save generation results to gallery ─────────────
                let result_text = maybe_save_video_to_gallery(
                    &result_text, fn_name, app, conv_id,
                );
                let result_text = maybe_save_image_to_gallery(
                    &result_text, fn_name, app, conv_id,
                ).await;

                // ── Buffer rich HTML and strip it from the LLM context ───
                // Keeps large SVG/HTML out of the model's context window.
                let result_text = if let Some(html) = extract_inline_html(&result_text) {
                    generated_html.push(html);
                    strip_html_from_result(&result_text)
                } else {
                    result_text
                };

                // ── Track generated PDFs as artifacts ────────────────────
                // Any tool whose result looks like {status:"created", path:"*.pdf"}
                // is treated as a PDF artifact source. Both `pdf_tools` and
                // `latex_pdf` packages publish this shape; relying on the shape
                // instead of the function name means new PDF-producing tools
                // get picked up automatically without code changes here.
                {
                    if let Ok(rv) = serde_json::from_str::<Value>(&result_text) {
                        let status = rv.get("status").and_then(|v| v.as_str()).unwrap_or("");
                        let path = rv.get("path").and_then(|v| v.as_str()).unwrap_or("");
                        if status == "created" && path.to_lowercase().ends_with(".pdf") {
                            let filename = rv
                                .get("filename")
                                .and_then(|v| v.as_str())
                                .unwrap_or("document.pdf")
                                .to_string();
                            let pages = rv.get("pages").and_then(|v| v.as_i64()).unwrap_or(0);
                            let engine = rv
                                .get("engine")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let mut payload = json!({
                                "path": path,
                                "filename": filename,
                                "pages": pages,
                            });
                            if !engine.is_empty() {
                                payload["engine"] = json!(engine);
                            }
                            let content = serde_json::to_string(&payload).unwrap_or_default();
                            log_event("info", format!(
                                "[executor] PDF created by '{}': {} ({} pages) → queuing artifact",
                                fn_name, filename, pages
                            ));
                            generated_pdfs.push(GeneratedPdf {
                                title: filename,
                                content,
                            });
                        }
                    }
                }

                // ── Track generated media for later markdown injection ────
                if let Ok(rv) = serde_json::from_str::<Value>(&result_text) {
                    let status = rv.get("status").and_then(|v| v.as_str()).unwrap_or("");
                    if status == "generated" {
                        let fn_lower = fn_name.to_lowercase();
                        let is_video = fn_lower.contains("generate_video") || fn_lower.contains("image_to_video");
                        let url_key = if is_video { "video_url" } else { "image_url" };
                        let kind = if is_video { "video" } else { "image" };
                        if let Some(url) = rv.get(url_key).and_then(|v| v.as_str()) {
                            let filename = rv
                                .get("filename")
                                .and_then(|v| v.as_str())
                                .unwrap_or("generated")
                                .to_string();
                            let gallery_id = rv
                                .get("gallery_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            generated_media.push(GeneratedMedia {
                                kind,
                                url: url.to_string(),
                                gallery_id,
                                filename,
                            });
                        }
                    }
                }

                let result_preview: String = result_text.chars().take(100).collect();
                log_event("info", format!(
                    "[executor] Tool '{}' result ({} chars): {}", fn_name, result_text.len(), result_preview
                ));

                // Emit result event to frontend and HTTP SSE clients
                let _ = app.emit(
                    "chat_tool_result",
                    json!({
                        "conversation_id": conv_id,
                        "tool_call_id": tc.id,
                        "function_name": fn_name,
                        "result": result_text,
                        "turn": turn,
                    }),
                );
                {
                    use tauri::Manager;
                    if let Some(st) = app.try_state::<crate::state::AppState>() {
                        let _ = st.event_tx.send(crate::api_server::events::ApiEvent::ChatToolResult {
                            conversation_id: conv_id.to_string(),
                            tool_call_id: tc.id.clone(),
                            result: result_text.clone(),
                        });
                    }
                }

                // ── Detect error-shaped tool results ────────────────────
                //
                // Tools that fail at runtime (rather than via MCP protocol)
                // return a JSON blob with a top-level `"error"` key, e.g.
                // `{"error":"LaTeX compilation failed", ...}`. Record them so
                // we can force a pinned retry once the dispatch loop ends.
                if let Some(err_msg) = extract_error_from_result(&result_text) {
                    log_event("warn", format!(
                        "[executor] Turn {} — tool '{}' returned an error result: {}",
                        turn, fn_name, err_msg.chars().take(160).collect::<String>()
                    ));
                    turn_tool_errors.push((fn_name.to_string(), err_msg));
                }

                // Append tool result as a "tool" role message
                messages.push((
                    format!("tool::{}", tc.id),
                    result_text,
                ));
            }

            // ── Post-dispatch: handle tool runtime errors ─────────────────
            //
            // At least one dispatched tool returned an error-shaped result.
            // Inject a corrective user message and pin `tool_choice` to that
            // tool for the next turn so the model retries with fixed args
            // instead of dropping into a "let me describe the fix" narration
            // (which has been observed in practice — see latex_pdf regression).
            if let Some((bad_tool, bad_err)) = turn_tool_errors.into_iter().next() {
                let attempts = self.tool_error_retries.fetch_add(1, Ordering::Relaxed) + 1;
                if attempts > MAX_TOOL_ERROR_RETRIES {
                    log_event("warn", format!(
                        "[executor] Turn {} — `{}` kept erroring ({}/{} retries). \
                         Letting the model respond normally on the next turn.",
                        turn, bad_tool, attempts, MAX_TOOL_ERROR_RETRIES
                    ));
                    self.tool_error_retries.store(0, Ordering::Relaxed);
                    prior_tool_error = None;
                    continue;
                }
                log_event("info", format!(
                    "[executor] Turn {} — pinning `{}` for retry (attempt {}/{}) after tool error.",
                    turn, bad_tool, attempts, MAX_TOOL_ERROR_RETRIES
                ));
                messages.push((
                    "user".to_string(),
                    format!(
                        "Your call to `{tool}` returned an error: {err}. \
                         Read the error carefully, then call `{tool}` again RIGHT NOW with \
                         corrected arguments that directly address the failure. \
                         Your next response MUST be a tool call — do NOT reply in prose, \
                         do NOT ask the user for clarification, do NOT describe what you are \
                         about to do. Emit the tool call now.",
                        tool = bad_tool,
                        err = bad_err,
                    ),
                ));
                pinned_tool_choice = Some(json!({
                    "type": "function",
                    "function": { "name": bad_tool },
                }));
                prior_tool_error = Some((bad_tool, bad_err));
            } else {
                // Clean turn — every tool result was healthy. Reset the
                // runtime-error retry counter so unrelated failures later in
                // the conversation get their own fresh budget.
                self.tool_error_retries.store(0, Ordering::Relaxed);
                prior_tool_error = None;
            }
            // Continue the loop for the next model turn
        }

        // Exceeded max turns — inform the user
        log_event("warn", format!("[executor] Exceeded maximum tool-call turns ({})", MAX_TOOL_TURNS));
        let _ = token_tx
            .send("[Max tool-call turns exceeded]".to_string())
            .await;
        let _ = token_tx.send("[DONE]".to_string()).await;
        Ok(())
    }

    /// Route a qualified tool name (`server_id__tool_name`) to the right MCP server,
    /// or to the built-in code_runner / comfyui when the prefix matches.
    async fn dispatch_tool_call(
        &self,
        qualified_name: &str,
        arguments: Value,
        app: &tauri::AppHandle,
        conv_id: &str,
    ) -> Result<String> {
        // ── Built-in code_runner dispatch ─────────────────────────────────
        if let Some(tool_name) = qualified_name.strip_prefix(&format!("{}__", CODE_RUNNER_SERVER_ID)) {
            return self.dispatch_code_runner(tool_name, arguments).await;
        }

        // ── MCP server dispatch ───────────────────────────────────────────
        // Use rfind so that server IDs containing "__" (e.g. "pkg__jellyfin")
        // are preserved: "pkg__jellyfin__get_recently_added" → ("pkg__jellyfin", "get_recently_added")
        let (server_id, tool_name) = if let Some(pos) = qualified_name.rfind("__") {
            (&qualified_name[..pos], &qualified_name[pos + 2..])
        } else {
            // Unqualified — try to find the server
            let sid = self
                .skills
                .find_server_for_tool(qualified_name)
                .await
                .with_context(|| format!("No MCP server found for tool '{}'", qualified_name))?;
            return Box::pin(self.dispatch_tool_call(
                &format!("{}__{}", sid, qualified_name),
                arguments,
                app,
                conv_id,
            ))
            .await;
        };

        let result = self
            .skills
            .call_tool(server_id, tool_name, arguments)
            .await?;

        if result.is_error {
            anyhow::bail!("Tool error: {}", result.text());
        }
        Ok(result.text())
    }

    /// Dispatch a built-in code_runner tool call.
    async fn dispatch_code_runner(&self, tool_name: &str, arguments: Value) -> Result<String> {
        match tool_name {
            "execute_code" => {
                let language = arguments
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("python");
                let code = arguments
                    .get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Only Python may be executed. Every other language is redirected
                // back to the LLM with an instruction to emit an artifact instead.
                if language != "python" {
                    let instruction = match language {
                        "html" | "htm" | "css" => format!(
                            "STOP — HTML/CSS must NOT be passed to execute_code.\n\
                             Output it as an artifact in your NEXT response (no tool call):\n\
                             <artifact type=\"html\" title=\"Page Title\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            code
                        ),
                        "javascript" | "typescript" | "js" | "ts" => format!(
                            "STOP — JavaScript/TypeScript cannot be executed here.\n\
                             Output it as a code artifact in your NEXT response (no tool call):\n\
                             <artifact type=\"code\" language=\"javascript\" title=\"Script\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            code
                        ),
                        "shell" | "bash" | "sh" | "zsh" | "powershell" | "ps1" => format!(
                            "STOP — Shell scripts cannot be executed here.\n\
                             Output it as a code artifact in your NEXT response (no tool call):\n\
                             <artifact type=\"code\" language=\"shell\" title=\"Shell Script\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            code
                        ),
                        other => format!(
                            "STOP — Only Python can be executed with execute_code. \
                             '{}' is not a supported execution language.\n\
                             If this is displayable code, output it as a code artifact in your \
                             NEXT response (no tool call):\n\
                             <artifact type=\"code\" language=\"{}\" title=\"Code\">\n{}\n</artifact>\n\
                             Do NOT call any tool.",
                            other, other, code
                        ),
                    };
                    log::warn!("[executor] Non-Python execute_code attempt (language={}) — redirecting to artifact", language);
                    return Ok(serde_json::to_string(&json!({
                        "error": "NOT_EXECUTABLE",
                        "instruction": instruction,
                    })).unwrap_or_default());
                }

                // ── Pre-flight guards ──────────────────────────────────────────
                // Guard 1: HTML/CSS content stored inside a Python variable.
                // Pattern: html_content = """<!DOCTYPE html>..."""
                // This never produces useful output and always either fails JSON
                // parsing (if large enough) or runs silently and returns nothing.
                let code_lower = code.to_lowercase();
                let has_html_signature = code_lower.contains("<!doctype")
                    || (code_lower.contains("<html") && code_lower.contains("</html>"));
                if has_html_signature && code.len() > 512 {
                    log::warn!(
                        "[executor] execute_code called with HTML embedded in Python ({} chars) — redirecting to artifact",
                        code.len()
                    );
                    return Ok(serde_json::to_string(&json!({
                        "error": "HTML_IN_PYTHON",
                        "instruction":
                            "STOP — You are embedding HTML inside a Python variable and passing it \
                             to execute_code. This is WRONG and will always fail or produce no output.\n\
                             Output the HTML DIRECTLY as an artifact in your NEXT response — \
                             do NOT call any tool:\n\
                             <artifact type=\"html\" title=\"Page Title\">\n\
                             <!DOCTYPE html>\n...(full HTML here)...\n\
                             </artifact>\n\
                             Write the complete artifact tag immediately. No tool call."
                    })).unwrap_or_default());
                }

                // Guard 2: shelling out to a LaTeX compiler from Python.
                // The user has the `latex_pdf` MCP package which exposes dedicated
                // tools (compile_latex, create_latex_pdf, render_equation,
                // create_math_document) that handle TeX engine discovery, lazy
                // Tectonic download, temp-dir management, and structured error
                // reporting. Calling pdflatex via subprocess bypasses all of that
                // and fails when no TeX distribution is on PATH.
                let latex_engine_invocation = {
                    // Detect any reference to a LaTeX engine binary together with
                    // a subprocess API. We look for the engine name *and* a
                    // subprocess/os call on either side of it — this avoids
                    // flagging benign code that merely mentions the word in a
                    // comment or string literal unrelated to execution.
                    let has_engine = [
                        "pdflatex",
                        "xelatex",
                        "lualatex",
                        "tectonic",
                        "latexmk",
                    ]
                    .iter()
                    .any(|needle| code_lower.contains(needle));
                    let has_subprocess_call = code_lower.contains("subprocess.")
                        || code_lower.contains("subprocess.run")
                        || code_lower.contains("subprocess.popen")
                        || code_lower.contains("subprocess.call")
                        || code_lower.contains("subprocess.check_")
                        || code_lower.contains("os.system(")
                        || code_lower.contains("os.popen(")
                        || code_lower.contains("shutil.which(\"pdflatex")
                        || code_lower.contains("shutil.which('pdflatex");
                    has_engine && has_subprocess_call
                };
                if latex_engine_invocation {
                    log::warn!(
                        "[executor] execute_code attempting to shell out to a LaTeX engine — \
                         redirecting to the latex_pdf MCP package"
                    );
                    return Ok(serde_json::to_string(&json!({
                        "error": "LATEX_VIA_SUBPROCESS",
                        "instruction":
                            "STOP — do NOT call execute_code to run pdflatex / xelatex / \
                             lualatex / tectonic / latexmk via subprocess. This machine may not \
                             have a TeX distribution on PATH and the subprocess call will fail \
                             with FileNotFoundError.\n\n\
                             Use the dedicated `latex_pdf` MCP tools instead — they handle \
                             engine discovery, lazy Tectonic auto-download, and error \
                             reporting for you:\n\
                             • compile_latex(source, filename)               — raw .tex passthrough (use this for a full \\documentclass document).\n\
                             • create_latex_pdf(filename, title, content)    — Markdown body + inline $..$ and display $$..$$ math.\n\
                             • create_math_document(filename, title, sections) — multi-section document with numbered equations and tables.\n\
                             • render_equation(equation, filename)           — single tightly-cropped equation PDF.\n\
                             • ensure_latex_engine()                         — one-shot warm-up if you want to absorb the first-call download.\n\n\
                             Call one of those tools NOW with the LaTeX source you were about \
                             to compile. Do not call execute_code.\n\n\
                             If the `latex_pdf` package is not installed in this environment, \
                             tell the user to install it from the Packages view — do NOT fall \
                             back to subprocess."
                    })).unwrap_or_default());
                }

                // Guard 3: oversized code (> 12 KB).
                // Large code blocks can cause llama-server JSON parse failures
                // when the argument string is truncated mid-generation.
                const MAX_CODE_BYTES: usize = 12_288;
                if code.len() > MAX_CODE_BYTES {
                    log::warn!(
                        "[executor] execute_code code too large ({} chars > {} limit) — redirecting",
                        code.len(), MAX_CODE_BYTES
                    );
                    return Ok(serde_json::to_string(&json!({
                        "error": "CODE_TOO_LARGE",
                        "instruction": format!(
                            "Your Python code is {} characters, which exceeds the safe limit of {} bytes.\n\
                             If you are generating file content (HTML, data, documents), output it DIRECTLY \
                             as an artifact instead — no tool call needed:\n\
                             <artifact type=\"html\" title=\"Title\">...content...</artifact>\n\
                             If this is genuine computational code, split it into smaller functions and \
                             call execute_code for each part separately.",
                            code.len(), MAX_CODE_BYTES
                        )
                    })).unwrap_or_default());
                }
                // ── End pre-flight guards ──────────────────────────────────────

                log::info!("[executor] Executing {} code ({} chars)", language, code.len());
                let run = crate::code_runner::execute_code(language, code).await?;
                log::info!(
                    "[executor] Code execution complete: exit_code={} stdout_len={} stderr_len={}",
                    run.exit_code, run.stdout.len(), run.stderr.len()
                );

                // Build result as a mutable JSON value so we can attach retry hints.
                let mut result_val = serde_json::to_value(&run).unwrap_or(json!({}));

                let hard_fail = run.exit_code != 0 || !run.stderr.is_empty();
                let soft_fail = !hard_fail && {
                    let out = run.stdout.to_ascii_lowercase();
                    out.contains("traceback") || out.contains("error:") || out.contains("exception")
                };

                if hard_fail {
                    // Increment the persistent retry counter for this executor session.
                    let attempt = self.code_exec_retries.fetch_add(1, Ordering::Relaxed) + 1;
                    log::warn!("[executor] Hard code failure (attempt {}/3) exit_code={}", attempt, run.exit_code);
                    if attempt <= 3 {
                        result_val["_retry_hint"] = json!(format!(
                            "FAILURE: Code exited with code {}. \
                             Analyze the error above, fix the code, and call execute_code again immediately. \
                             This is attempt {}/3. DO NOT describe the error — rewrite and retry.",
                            run.exit_code, attempt
                        ));
                    } else {
                        result_val["_retry_hint"] = json!(
                            "MAX_RETRIES_REACHED: 3 code execution attempts have failed. \
                             Do NOT call execute_code again. \
                             Summarize what went wrong and provide your best explanation."
                        );
                    }
                } else if soft_fail {
                    log::info!("[executor] Soft code failure detected in stdout (caught exception?)");
                    result_val["_retry_hint"] = json!(
                        "WARNING: The output suggests a runtime error was caught and printed. \
                         If the result is wrong or an error occurred, fix the code and call \
                         execute_code again (max 3 total attempts). \
                         Always use sys.exit(1) on failure so errors are reliably detected."
                    );
                }

                Ok(result_val.to_string())
            }
            "list_recent_artifacts" => {
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5) as usize;

                let db = self
                    .code_runner_db
                    .as_ref()
                    .context("code_runner_db not initialised")?;
                let conv_id = self
                    .code_runner_conv_id
                    .as_deref()
                    .context("code_runner_conv_id not initialised")?;

                let artifacts =
                    crate::code_runner::list_recent_artifacts(db, conv_id, limit)?;
                Ok(serde_json::to_string(&artifacts).unwrap_or_default())
            }
            other => anyhow::bail!("Unknown code_runner tool: '{}'", other),
        }
    }

}

// ── Intent scoring + per-turn tool scoping helpers ────────────────────────────
//
// These functions power `SkillsExecutor::scope_tools_for_turn`. They live at
// module scope (not inside the impl) so they're trivially unit-testable.

/// Concatenate the last `n` user messages into a single lowercased string
/// suitable for keyword scoring. Walks the history in reverse so we prefer
/// the most recent user intent over stale context.
fn collect_recent_user_text(messages: &[(String, String)], n: usize) -> String {
    messages
        .iter()
        .rev()
        .filter(|(role, _)| role == "user")
        .take(n)
        .map(|(_, content)| content.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Return the `server_id` of the tool that fulfilled the most recent assistant
/// tool call, so follow-up turns keep that server in scope ("sticky tools").
///
/// The executor stores assistant tool-call turns as a serialised JSON object
/// of shape `{"role":"assistant","content":...,"tool_calls":[{"function":{"name":"sid__tool"}}]}`,
/// so we parse it back out.
fn last_assistant_tool_server(messages: &[(String, String)]) -> Option<String> {
    for (role, content) in messages.iter().rev() {
        if role != "assistant" {
            continue;
        }
        if !content.contains("tool_calls") {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        let first_name = parsed
            .get("tool_calls")
            .and_then(|tcs| tcs.as_array())
            .and_then(|arr| arr.first())
            .and_then(|tc| tc.get("function"))
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str());
        if let Some(name) = first_name {
            return Some(extract_server_id(name));
        }
    }
    None
}

/// Split a qualified tool name (`server_id__tool_name`) on the final `__`
/// so server IDs that themselves contain `__` (none today, but future-proof)
/// are preserved.
fn extract_server_id(qualified_name: &str) -> String {
    qualified_name
        .rfind("__")
        .map(|pos| qualified_name[..pos].to_string())
        .unwrap_or_else(|| qualified_name.to_string())
}

/// Strong LaTeX intent means the user is asking for LaTeX / PDF / math output
/// directly. When detected, `code_runner` is dropped from the scoped tool list
/// so the model physically cannot reach for `subprocess.run(['pdflatex', ...])`
/// as a shortcut past the dedicated `latex_pdf` tools.
fn has_strong_latex_intent(text_lower: &str) -> bool {
    text_lower.contains("latex")
        || text_lower.contains("\\documentclass")
        || text_lower.contains("\\begin{")
        || (text_lower.contains("pdf")
            && (text_lower.contains("equation")
                || text_lower.contains("math")
                || text_lower.contains("formula")
                || text_lower.contains("theorem")))
        || text_lower.contains("tex document")
        || text_lower.contains(".tex ")
}

/// Keyword seed per server. Missing entries return `None` and score 0 by
/// default (they'll only be kept via the `any_hit=false` fallback or as
/// always-on utilities).
fn server_keywords(server_id: &str) -> Option<&'static [&'static str]> {
    match server_id {
        "latex_pdf" => Some(&[
            "latex", "pdf", "equation", "math", "\\", "formula", "theorem",
            "tex", "integral", "derivative", "matrix", "documentation",
        ]),
        "pdf_tools" => Some(&["pdf", "report", "merge", "split", "extract"]),
        "code_runner" => Some(&[
            "python", "run", "execute", "script", "calculate", "compute",
            "analyze", "plot",
        ]),
        "file_ops" => Some(&[
            "file", "write", "read", "save", "delete", "folder", "directory",
        ]),
        "rich_responses" => Some(&[
            "chart", "table", "card", "timeline", "dashboard", "graph",
            "summary",
        ]),
        "currency_rates" => Some(&[
            "currency", "exchange", "forex", "usd", "eur", "rate",
            "conversion",
        ]),
        "jellyfin" => Some(&[
            "jellyfin", "movie", "tv", "show", "media library", "episode",
            "series",
        ]),
        "comfyui_image" => Some(&[
            "image", "draw", "picture", "render", "illustration", "portrait",
        ]),
        "comfyui_image_edit" => Some(&[
            "edit image", "modify image", "inpaint", "alter image", "retouch",
        ]),
        "comfyui_video" => Some(&[
            "video", "animate", "motion", "clip",
        ]),
        "comfyui_img2video" => Some(&[
            "video from image", "animate image", "img2video", "image to video",
        ]),
        _ => None,
    }
}

/// Count how many of a server's seed keywords appear in the recent user text.
fn score_server(server_id: &str, recent_user_text_lower: &str) -> i32 {
    let Some(kws) = server_keywords(server_id) else {
        return 0;
    };
    kws.iter()
        .filter(|kw| recent_user_text_lower.contains(&kw.to_lowercase()))
        .count() as i32
}

/// Utility servers that are kept in scope even when nothing else matches.
/// `code_runner` is excluded under strong LaTeX intent — see the LaTeX guard
/// in `dispatch_code_runner` for the rationale.
fn is_always_on(server_id: &str, strong_latex: bool) -> bool {
    match server_id {
        "file_ops" => true,
        "code_runner" => !strong_latex,
        _ => false,
    }
}

/// Token budget: drop the lowest-scored servers iteratively until the
/// serialised tool list plus the recent message history fits inside the
/// `n_ctx * CTX_BUDGET_FRACTION` envelope.
///
/// Tools with the HIGHEST score are preserved first; ties go to
/// `is_always_on` servers (so the generic utilities survive). The result
/// retains the natural ordering of the input — we never reorder tools the
/// model might have already seen, only remove whole server groups.
fn budget_trim(
    mut kept: Vec<(String, Vec<ToolDefinition>, i32)>,
    messages: &[(String, String)],
    n_ctx: usize,
) -> Vec<ToolDefinition> {
    if kept.is_empty() {
        return Vec::new();
    }

    let budget = ((n_ctx as f32) * CTX_BUDGET_FRACTION) as usize;
    let history_tokens: usize = messages
        .iter()
        .map(|(_, c)| estimate_tokens(c))
        .sum();

    // Sort candidates-for-eviction by ascending (score, always_on_flag) so
    // the first one we pop is the LEAST relevant server.
    kept.sort_by(|a, b| {
        let a_always = is_always_on(&a.0, false) as i32;
        let b_always = is_always_on(&b.0, false) as i32;
        a.2.cmp(&b.2).then_with(|| a_always.cmp(&b_always))
    });

    loop {
        let tools_tokens: usize = kept
            .iter()
            .flat_map(|(_, ts, _)| ts.iter())
            .map(|t| estimate_tokens_for_tool(t))
            .sum();
        if tools_tokens + history_tokens <= budget || kept.len() <= 1 {
            break;
        }
        // Pop the least-relevant group.
        let (removed_sid, removed_tools, _score) = kept.remove(0);
        log::info!(
            "[executor] budget_trim: dropped server '{}' ({} tools) to fit \
             context budget (budget={}, history={}, tools_before_drop={})",
            removed_sid,
            removed_tools.len(),
            budget,
            history_tokens,
            tools_tokens
        );
    }

    // Restore deterministic order by server_id for stable prompts.
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    kept.into_iter().flat_map(|(_, ts, _)| ts).collect()
}

/// Rough char→token estimate (same heuristic as `chat.rs::estimate_tokens`).
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Approximate the token cost of a single serialised tool definition.
/// We measure the JSON representation because that's what actually lands in
/// the request body.
fn estimate_tokens_for_tool(t: &ToolDefinition) -> usize {
    let serialised = serde_json::to_string(t).unwrap_or_default();
    estimate_tokens(&serialised)
}

// ── Tool-call argument validation ────────────────────────────────────────────
//
// Replaces the old `unwrap_or(json!({}))` that silently swallowed truncated
// tool-call JSON. Returns the parsed arguments on success or a human-readable
// reason string on failure, which the executor then surfaces as a corrective
// message to the model.

/// Parse `args_json` and verify that every field listed as `required` in the
/// matching tool's `parameters` schema is present and non-null.
///
/// Returns:
/// * `Ok(parsed_value)` — args are valid and safe to dispatch.
/// * `Err(reason)`      — either the JSON is malformed or required fields are
///                        missing; caller should inject a corrective message
///                        and retry.
fn validate_tool_args(
    fn_name: &str,
    args_json: &str,
    tools: &[ToolDefinition],
) -> std::result::Result<Value, String> {
    // Treat empty/whitespace-only strings as "{}" rather than a parse error —
    // some chat templates emit that for zero-arg tools.
    let trimmed = args_json.trim();
    let parsed: Value = if trimmed.is_empty() {
        json!({})
    } else {
        serde_json::from_str(trimmed).map_err(|e| {
            format!(
                "arguments were not valid JSON: {} (received {} chars)",
                e,
                trimmed.len()
            )
        })?
    };

    if !parsed.is_object() {
        return Err(format!(
            "arguments must be a JSON object, got: {}",
            match &parsed {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            }
        ));
    }

    let Some(required) = required_fields_for(fn_name, tools) else {
        // No schema registered for this tool (native tools may omit) — accept.
        return Ok(parsed);
    };

    let obj = parsed.as_object().unwrap();
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|field| matches!(obj.get(*field), None | Some(Value::Null)))
        .collect();

    if missing.is_empty() {
        Ok(parsed)
    } else {
        Err(format!(
            "missing required field(s): {}",
            missing.join(", ")
        ))
    }
}

/// Inspect a tool's stringified JSON result and return the human-readable
/// error message when the result represents an application-level failure.
///
/// Application-level failures are those where the tool process exited
/// cleanly but returned `{"error": "..."}` (or `{"error": {...}}`) to signal
/// that it couldn't do what was asked. The Python `latex_pdf` tools, for
/// example, return `{"error": "LaTeX compilation failed", ...}` on a compile
/// failure. MCP-level errors (`is_error: true`) are already surfaced via
/// `anyhow::bail!` in `dispatch_tool_call`, which the caller wraps as
/// `{"error": "Tool '...' error: ..."}` — so both paths land here.
///
/// Returns `None` when `result_text` is not parseable as a JSON object, or
/// when the parsed object has no top-level `error` key.
fn extract_error_from_result(result_text: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(result_text).ok()?;
    let obj = parsed.as_object()?;
    let err = obj.get("error")?;
    let msg = match err {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    // Keep the carried message short — the full tool result is already in
    // the conversation history as a `tool` role message; this value is only
    // used in the corrective prompt injected into the next turn.
    Some(msg.chars().take(400).collect::<String>())
}

/// Return the list of required fields declared in the tool's input_schema.
/// `None` when the schema has no `required` array.
fn required_fields_for<'a>(
    fn_name: &str,
    tools: &'a [ToolDefinition],
) -> Option<Vec<&'a str>> {
    let tool = tools.iter().find(|t| t.function.name == fn_name)?;
    let arr = tool
        .function
        .parameters
        .get("required")
        .and_then(|v| v.as_array())?;
    Some(
        arr.iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<&str>>(),
    )
}

/// If `result_text` looks like a successful video generation result (has `video_url`
/// and `status: "generated"`), persist it to the gallery and inject `gallery_id` into
/// the returned JSON so the frontend can reference it.
///
/// Videos are stored by URL reference (not base64) because they can be very large.
/// The `image_data` column holds the video URL; the `mime_type` distinguishes
/// video entries from image entries.
fn maybe_save_video_to_gallery(
    result_text: &str,
    fn_name: &str,
    app: &tauri::AppHandle,
    conv_id: &str,
) -> String {
    // Only process tool calls that look like video generation.
    let fn_lower = fn_name.to_lowercase();
    if !fn_lower.contains("generate_video") && !fn_lower.contains("image_to_video") {
        return result_text.to_string();
    }

    let mut result_val: Value = match serde_json::from_str(result_text) {
        Ok(v) => v,
        Err(_) => return result_text.to_string(),
    };

    let status = result_val.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let video_url = result_val.get("video_url").and_then(|v| v.as_str()).unwrap_or("");
    if status != "generated" || video_url.is_empty() {
        return result_text.to_string();
    }

    let filename = result_val.get("filename").and_then(|v| v.as_str()).unwrap_or("video.mp4").to_string();
    let prompt = result_val.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let width = result_val.get("width").and_then(|v| v.as_i64());
    let height = result_val.get("height").and_then(|v| v.as_i64());

    let mime = if filename.to_lowercase().ends_with(".gif") {
        "image/gif"
    } else if filename.to_lowercase().ends_with(".webm") {
        "video/webm"
    } else {
        "video/mp4"
    };

    let gid = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    {
        let db = state.db.lock().unwrap();
        match db.conn.execute(
            "INSERT INTO gallery_images \
             (id, conversation_id, source, filename, image_data, mime_type, \
              prompt, width, height, created_at) \
             VALUES (?1, ?2, 'generated', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            sql_params![
                gid,
                conv_id,
                filename,
                video_url,
                mime,
                prompt,
                width,
                height,
                now
            ],
        ) {
            Ok(_) => {
                log::info!(
                    "[video-gallery] Saved video to gallery: id={} conv={} file={}",
                    gid, conv_id, filename
                );
                result_val["gallery_id"] = json!(gid);
            }
            Err(e) => {
                log::error!(
                    "[video-gallery] Failed to save video to gallery: {} (conv_id={}, filename={})",
                    e, conv_id, filename
                );
            }
        }
    }

    // Emit gallery_updated so the frontend refreshes
    if result_val.get("gallery_id").is_some() {
        let _ = app.emit(
            "gallery_updated",
            json!({ "conversation_id": conv_id }),
        );
        if let Some(st) = app.try_state::<crate::state::AppState>() {
            let _ = st.event_tx.send(crate::api_server::events::ApiEvent::GalleryUpdated {
                conversation_id: conv_id.to_string(),
            });
        }
    }

    serde_json::to_string(&result_val).unwrap_or_else(|_| result_text.to_string())
}

/// If `result_text` looks like a successful image generation or editing result
/// (has `image_url` and `status: "generated"`), downloads the image, writes it
/// to `{data_dir}/gallery/{id}.ext` on disk, and stores the file path in the DB.
/// This avoids base64 bloat in SQLite and makes images loadable via Tauri's
/// `asset://` protocol — no HTTP round-trip through the WebView.
async fn maybe_save_image_to_gallery(
    result_text: &str,
    fn_name: &str,
    app: &tauri::AppHandle,
    conv_id: &str,
) -> String {
    let fn_lower = fn_name.to_lowercase();
    if !fn_lower.contains("generate_image") && !fn_lower.contains("edit_image") {
        return result_text.to_string();
    }

    let mut result_val: Value = match serde_json::from_str(result_text) {
        Ok(v) => v,
        Err(_) => return result_text.to_string(),
    };

    let status = result_val.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let image_url = result_val.get("image_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if status != "generated" || image_url.is_empty() {
        return result_text.to_string();
    }

    if result_val.get("gallery_id").is_some() {
        return result_text.to_string();
    }

    let filename = result_val
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("image.png")
        .to_string();
    let prompt = result_val.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let width = result_val.get("width").and_then(|v| v.as_i64());
    let height = result_val.get("height").and_then(|v| v.as_i64());

    let ext = if filename.to_lowercase().ends_with(".webp") {
        "webp"
    } else if filename.to_lowercase().ends_with(".jpg") || filename.to_lowercase().ends_with(".jpeg") {
        "jpg"
    } else {
        "png"
    };
    let mime = match ext {
        "webp" => "image/webp",
        "jpg" => "image/jpeg",
        _ => "image/png",
    };

    let gid = uuid::Uuid::new_v4().to_string();

    use tauri::Manager;
    let state = app.state::<crate::state::AppState>();
    let gallery_dir = state.data_dir.join("gallery");
    let _ = std::fs::create_dir_all(&gallery_dir);
    let dest_path = gallery_dir.join(format!("{}.{}", gid, ext));

    // Download with timeout + retry, write directly to disk (no base64).
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut file_path_str = String::new();
    let mut fallback_image_data = String::new();

    if image_url.starts_with("http://") || image_url.starts_with("https://") {
        let mut downloaded = false;
        for attempt in 1..=3u32 {
            match client.get(&image_url).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => {
                        match std::fs::write(&dest_path, &bytes) {
                            Ok(_) => {
                                file_path_str = dest_path.to_string_lossy().to_string();
                                downloaded = true;
                                log::info!(
                                    "[image-gallery] Saved {} bytes to {} (attempt {})",
                                    bytes.len(), file_path_str, attempt
                                );
                            }
                            Err(e) => {
                                log::warn!("[image-gallery] fs::write failed: {} (attempt {})", e, attempt);
                            }
                        }
                        break;
                    }
                    Err(e) => {
                        log::warn!("[image-gallery] read bytes failed: {} (attempt {})", e, attempt);
                    }
                },
                Err(e) => {
                    log::warn!("[image-gallery] download failed: {} (attempt {})", e, attempt);
                }
            }
            if attempt < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
            }
        }
        if !downloaded {
            log::warn!("[image-gallery] All 3 download attempts failed, storing URL as fallback");
            fallback_image_data = image_url.clone();
        }
    } else {
        fallback_image_data = image_url.clone();
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mobile_port = state.settings.lock().unwrap().mobile_api_port;

    // image_data is empty when file_path is set; holds URL/base64 only as legacy fallback.
    let db_image_data = if file_path_str.is_empty() { &fallback_image_data } else { "" };
    let db_file_path: Option<&str> = if file_path_str.is_empty() { None } else { Some(&file_path_str) };

    {
        let db = state.db.lock().unwrap();
        match db.conn.execute(
            "INSERT INTO gallery_images \
             (id, conversation_id, source, filename, image_data, mime_type, \
              prompt, width, height, created_at, file_path) \
             VALUES (?1, ?2, 'generated', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            sql_params![
                gid,
                conv_id,
                filename,
                db_image_data,
                mime,
                prompt,
                width,
                height,
                now,
                db_file_path
            ],
        ) {
            Ok(_) => {
                log::info!(
                    "[image-gallery] Saved to gallery: id={} conv={} file_path={:?}",
                    gid, conv_id, db_file_path
                );
                let local_url = format!("http://localhost:{}/images/{}", mobile_port, gid);
                result_val["gallery_id"] = json!(gid);
                result_val["image_url"] = json!(local_url);
                if let Some(fp) = db_file_path {
                    result_val["file_path"] = json!(fp);
                }
            }
            Err(e) => {
                log::error!(
                    "[image-gallery] Failed to insert gallery row: {} (conv_id={}, filename={})",
                    e, conv_id, filename
                );
            }
        }
    }

    if result_val.get("gallery_id").is_some() {
        let _ = app.emit(
            "gallery_updated",
            json!({ "conversation_id": conv_id }),
        );
        if let Some(st) = app.try_state::<crate::state::AppState>() {
            let _ = st.event_tx.send(crate::api_server::events::ApiEvent::GalleryUpdated {
                conversation_id: conv_id.to_string(),
            });
        }
    }

    serde_json::to_string(&result_val).unwrap_or_else(|_| result_text.to_string())
}

// ── Gallery URL → temp file resolution ───────────────────────────────────────
//
// When the executor is about to dispatch a tool call, any JSON argument string
// that matches `http://localhost:{port}/images/{gallery_id}` is replaced with
// an absolute path to a temp file containing the raw image bytes from SQLite.
//
// This avoids the Python subprocess needing to make HTTP requests to the local
// API server, which fails on Windows because the server binds to `0.0.0.0`
// (IPv4 only) but Python may resolve `localhost` to `::1` (IPv6).
//
// The returned Vec<String> holds the temp file paths; they are deleted when the
// Vec is dropped (via a wrapper that removes the files on drop).

struct TempFile(String);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn resolve_gallery_image_urls(
    mut args: Value,
    state: &crate::state::AppState,
) -> (Value, Vec<TempFile>) {
    let mut temp_files: Vec<TempFile> = Vec::new();

    let port = state.settings.lock().unwrap().mobile_api_port;
    let prefix = format!("http://localhost:{}/images/", port);

    // Walk every string value in the JSON args.
    walk_and_replace(&mut args, &prefix, state, &mut temp_files);

    (args, temp_files)
}

fn walk_and_replace(
    val: &mut Value,
    prefix: &str,
    state: &crate::state::AppState,
    temp_files: &mut Vec<TempFile>,
) {
    match val {
        Value::String(s) => {
            if s.starts_with(prefix) {
                let gallery_id = &s[prefix.len()..];
                if let Some(path) = gallery_id_to_temp_file(gallery_id, state) {
                    *s = path.clone();
                    temp_files.push(TempFile(path));
                }
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                walk_and_replace(v, prefix, state, temp_files);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                walk_and_replace(v, prefix, state, temp_files);
            }
        }
        _ => {}
    }
}

fn gallery_id_to_temp_file(gallery_id: &str, state: &crate::state::AppState) -> Option<String> {
    // Load image_data and mime_type from the gallery.
    let (image_data, mime_type, filename): (String, String, String) = {
        let db = state.db.lock().unwrap();
        db.conn.query_row(
            "SELECT image_data, mime_type, filename FROM gallery_images WHERE id = ?1",
            rusqlite::params![gallery_id],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            )),
        ).ok()?
    };

    // If `image_data` is base64 — decode to bytes.
    // If it's still a URL (legacy), we can't help here; skip.
    if image_data.starts_with("http://") || image_data.starts_with("https://") {
        log::warn!(
            "[gallery-resolve] Gallery entry {} has a URL instead of base64 data; skipping temp-file resolution",
            gallery_id
        );
        return None;
    }

    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(&image_data).ok()?;

    // Determine extension from mime type.
    let ext = match mime_type.as_str() {
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        _ => {
            // Fall back to the filename's extension.
            std::path::Path::new(&filename)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| if e.starts_with('.') { e } else { "" })
                .unwrap_or(".png")
        }
    };

    let tmp_dir = std::env::temp_dir();
    let tmp_name = format!("xandsuite_img_{}{}", gallery_id, ext);
    let tmp_path = tmp_dir.join(&tmp_name);

    match std::fs::write(&tmp_path, &bytes) {
        Ok(_) => {
            log::info!(
                "[gallery-resolve] Wrote gallery image {} to {}",
                gallery_id,
                tmp_path.display()
            );
            Some(tmp_path.to_string_lossy().into_owned())
        }
        Err(e) => {
            log::error!("[gallery-resolve] Failed to write temp file: {}", e);
            None
        }
    }
}

// ── Built-in tool schema definitions ─────────────────────────────────────────

fn code_runner_execute_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: format!("{}__execute_code", CODE_RUNNER_SERVER_ID),
            description: "Run Python code in a real sandboxed subprocess and return stdout, \
                           stderr, exit code, and wall-clock time. \
                           ONLY Python is supported — do NOT pass any other language to this tool. \
                           For HTML/CSS pages use: <artifact type=\"html\" title=\"...\">...</artifact>. \
                           For JavaScript/TypeScript use: <artifact type=\"code\" language=\"javascript\" title=\"...\">...</artifact>. \
                           For shell scripts use: <artifact type=\"code\" language=\"shell\" title=\"...\">...</artifact>. \
                           NEVER use this tool to shell out to a LaTeX engine (pdflatex, xelatex, \
                           lualatex, tectonic, latexmk) via subprocess — the call will fail when \
                           no TeX distribution is on PATH. For LaTeX / PDF generation with math, \
                           use the dedicated `latex_pdf` MCP tools instead: `compile_latex` (raw \
                           .tex), `create_latex_pdf` (Markdown + math), `create_math_document` \
                           (multi-section with equations and tables), `render_equation` (single \
                           equation PDF), or `ensure_latex_engine` (pre-warm the engine cache). \
                           Those tools handle engine discovery and automatically download \
                           Tectonic if needed — they must be used instead of subprocess. \
                           Call this tool only when the user asks you to run, execute, test, or \
                           verify Python code, or whenever you need to show real computed output. \
                           IMPORTANT: Always signal failures with `sys.exit(1)` — do NOT silently \
                           catch exceptions and print them; raise them so errors are reliably \
                           detected and retried. \
                           If a result has `_retry_hint` in the response, read it and act on it."
                .to_string(),
            parameters: json!({
                "type": "object",
                "required": ["language", "code"],
                "properties": {
                    "language": {
                        "type": "string",
                        "enum": ["python"],
                        "description": "Must be 'python'. This is the only supported execution language. For all other languages output an artifact tag instead of calling this tool."
                    },
                    "code": {
                        "type": "string",
                        "description": "The Python source code to run. Write it exactly as you would in a .py file."
                    }
                }
            }),
        },
    }
}

/// Parse a rich-response tool result and return the `html` field value if the
/// result has `"display": "inline_html"`. Returns `None` for all other results.
fn extract_inline_html(result: &str) -> Option<String> {
    let v: Value = serde_json::from_str(result).ok()?;
    if v.get("display").and_then(|d| d.as_str()) != Some("inline_html") {
        return None;
    }
    v.get("html").and_then(|h| h.as_str()).map(|s| s.to_string())
}

/// Return the tool result JSON with the `html` field replaced by a short
/// acknowledgement so the LLM context stays small.
fn strip_html_from_result(result: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(result) else {
        return result.to_string();
    };
    if let Some(obj) = v.as_object_mut() {
        obj.insert("html".to_string(), json!("[rendered inline]"));
    }
    serde_json::to_string(&v).unwrap_or_else(|_| result.to_string())
}

fn code_runner_list_artifacts_tool() -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: format!("{}__list_recent_artifacts", CODE_RUNNER_SERVER_ID),
            description: "List recent artifacts (code, markdown, html, text) created in this \
                           conversation. Returns title, type, language, and a content preview. \
                           Use this to find and re-run code from earlier in the conversation."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "number",
                        "description": "Maximum number of artifacts to return (default 5, max 20)."
                    }
                }
            }),
        },
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────────
//
// These tests cover the pure helpers that drive scoping, token budgeting, and
// tool-call argument validation. They run without a Tauri app handle, a live
// llama-server, or the skills manager, so `cargo test -p xandsuite_lib` will
// exercise them in isolation.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tool(name: &str, required: &[&str]) -> ToolDefinition {
        let mut properties = serde_json::Map::new();
        for field in required {
            properties.insert(
                (*field).to_string(),
                json!({ "type": "string" }),
            );
        }
        ToolDefinition {
            kind: "function".to_string(),
            function: FunctionDef {
                name: name.to_string(),
                description: format!("Test tool {}", name),
                parameters: json!({
                    "type": "object",
                    "required": required,
                    "properties": Value::Object(properties),
                }),
            },
        }
    }

    // ── extract_server_id ───────────────────────────────────────────────────

    #[test]
    fn extract_server_id_splits_on_last_double_underscore() {
        assert_eq!(extract_server_id("latex_pdf__compile_latex"), "latex_pdf");
        assert_eq!(
            extract_server_id("pkg__latex_pdf__create_math_document"),
            "pkg__latex_pdf"
        );
        assert_eq!(extract_server_id("no_separator"), "no_separator");
    }

    // ── has_strong_latex_intent ─────────────────────────────────────────────

    #[test]
    fn has_strong_latex_intent_detects_common_phrasings() {
        assert!(has_strong_latex_intent("please write this in latex"));
        assert!(has_strong_latex_intent("\\documentclass{article}"));
        assert!(has_strong_latex_intent("i need a pdf with an equation"));
        assert!(has_strong_latex_intent("render math as pdf"));
    }

    #[test]
    fn has_strong_latex_intent_rejects_unrelated_prompts() {
        assert!(!has_strong_latex_intent("can you draw me a cat image"));
        assert!(!has_strong_latex_intent("just a pdf report of sales"));
        assert!(!has_strong_latex_intent("run python to compute pi"));
    }

    // ── score_server ────────────────────────────────────────────────────────

    #[test]
    fn score_server_matches_relevant_keywords() {
        let text = "please generate a latex pdf with a matrix equation";
        assert!(score_server("latex_pdf", text) >= 3);
        assert_eq!(score_server("jellyfin", text), 0);
    }

    #[test]
    fn score_server_unknown_server_scores_zero() {
        assert_eq!(score_server("made_up_server", "any text"), 0);
    }

    // ── is_always_on ────────────────────────────────────────────────────────

    #[test]
    fn is_always_on_drops_code_runner_when_latex() {
        assert!(is_always_on("file_ops", false));
        assert!(is_always_on("file_ops", true));
        assert!(is_always_on("code_runner", false));
        assert!(!is_always_on("code_runner", true));
        assert!(!is_always_on("latex_pdf", false));
    }

    // ── estimate_tokens / estimate_tokens_for_tool ──────────────────────────

    #[test]
    fn estimate_tokens_is_proportional_to_length() {
        assert_eq!(estimate_tokens(""), 0);
        assert!(estimate_tokens("hello world") > 0);
        let short = estimate_tokens("abcd");
        let long = estimate_tokens(&"abcd".repeat(100));
        assert!(long > short * 50);
    }

    #[test]
    fn estimate_tokens_for_tool_nonzero() {
        let t = make_tool("pkg__latex_pdf__compile_latex", &["source", "filename"]);
        assert!(estimate_tokens_for_tool(&t) > 0);
    }

    // ── budget_trim ────────────────────────────────────────────────────────

    #[test]
    fn budget_trim_returns_empty_when_input_empty() {
        let out = budget_trim(Vec::new(), &[], 4096);
        assert!(out.is_empty());
    }

    #[test]
    fn budget_trim_keeps_at_least_one_server_when_over_budget() {
        let big_tool = make_tool(
            "latex_pdf__create_math_document",
            &["filename", "title", "sections"],
        );
        let small_tool = make_tool("file_ops__read_file", &["path"]);

        let kept = vec![
            ("latex_pdf".to_string(), vec![big_tool.clone()], 5),
            ("file_ops".to_string(), vec![small_tool.clone()], 0),
        ];

        // n_ctx=32 ⇒ budget≈27 tokens, well below any tool's serialised size:
        // the loop must stop at len==1 rather than dropping everything.
        let out = budget_trim(kept, &[], 32);
        assert_eq!(out.len(), 1, "budget_trim must leave a non-empty tool set");
    }

    #[test]
    fn budget_trim_preserves_everything_when_fits() {
        let t1 = make_tool("latex_pdf__render_equation", &["equation", "filename"]);
        let t2 = make_tool("file_ops__read_file", &["path"]);
        let kept = vec![
            ("latex_pdf".to_string(), vec![t1], 3),
            ("file_ops".to_string(), vec![t2], 1),
        ];
        let out = budget_trim(kept, &[], 32_768);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn budget_trim_drops_lowest_scored_first() {
        // Three servers with known scores; sized so we can compute a budget
        // that forces exactly one drop regardless of Rust's exact string
        // serialisation width (the drop order is what we care about).
        let latex = make_tool("latex_pdf__compile_latex", &["source", "filename"]);
        let jellyfin = make_tool("jellyfin__search_media", &["query"]);
        let file_ops = make_tool("file_ops__read_file", &["path"]);

        let total: usize =
            [&latex, &jellyfin, &file_ops].iter().map(|t| estimate_tokens_for_tool(t)).sum();
        let jf_tokens = estimate_tokens_for_tool(&jellyfin);

        // Pick n_ctx so that CTX_BUDGET_FRACTION * n_ctx lands BETWEEN
        // (total - jf_tokens) and (total - 1) — dropping jellyfin should
        // therefore be sufficient to come under budget.
        let target_budget = total - jf_tokens / 2;
        let n_ctx = ((target_budget as f32) / CTX_BUDGET_FRACTION).ceil() as usize + 1;

        let kept = vec![
            ("latex_pdf".to_string(), vec![latex], 5),
            ("jellyfin".to_string(), vec![jellyfin], 0),
            ("file_ops".to_string(), vec![file_ops], 0),
        ];
        let out = budget_trim(kept, &[], n_ctx);

        let kept_names: Vec<String> =
            out.iter().map(|t| t.function.name.clone()).collect();
        assert!(
            !kept_names.iter().any(|n| n.starts_with("jellyfin__")),
            "jellyfin (lowest score, not always-on) should be dropped first — got {:?}",
            kept_names
        );
        assert!(
            kept_names.iter().any(|n| n.starts_with("latex_pdf__")),
            "latex_pdf (highest score) must survive — got {:?}",
            kept_names
        );
    }

    // ── validate_tool_args ──────────────────────────────────────────────────

    #[test]
    fn validate_tool_args_accepts_well_formed_args() {
        let tools = vec![make_tool(
            "latex_pdf__compile_latex",
            &["source", "filename"],
        )];
        let out = validate_tool_args(
            "latex_pdf__compile_latex",
            r#"{"source":"\\documentclass{article}","filename":"out.pdf"}"#,
            &tools,
        );
        assert!(out.is_ok(), "expected Ok, got {:?}", out);
    }

    #[test]
    fn validate_tool_args_empty_string_treated_as_empty_object() {
        let tools = vec![make_tool("latex_pdf__ensure_latex_engine", &[])];
        let out = validate_tool_args("latex_pdf__ensure_latex_engine", "", &tools);
        assert!(out.is_ok());
    }

    #[test]
    fn validate_tool_args_empty_object_fails_when_required_missing() {
        let tools = vec![make_tool(
            "latex_pdf__create_math_document",
            &["filename", "title", "sections"],
        )];
        let err = validate_tool_args(
            "latex_pdf__create_math_document",
            "{}",
            &tools,
        )
        .expect_err("expected missing-field error");
        assert!(err.contains("filename"));
        assert!(err.contains("title"));
        assert!(err.contains("sections"));
    }

    #[test]
    fn validate_tool_args_null_required_field_is_rejected() {
        let tools = vec![make_tool(
            "latex_pdf__render_equation",
            &["equation", "filename"],
        )];
        let err = validate_tool_args(
            "latex_pdf__render_equation",
            r#"{"equation":"x^2","filename":null}"#,
            &tools,
        )
        .expect_err("expected null-field error");
        assert!(err.contains("filename"));
    }

    #[test]
    fn validate_tool_args_malformed_json_returns_error() {
        let tools = vec![make_tool(
            "latex_pdf__compile_latex",
            &["source", "filename"],
        )];
        let err = validate_tool_args(
            "latex_pdf__compile_latex",
            r#"{"source":"foo"#,
            &tools,
        )
        .expect_err("expected parse error");
        assert!(err.to_lowercase().contains("json"));
    }

    #[test]
    fn validate_tool_args_non_object_rejected() {
        let tools = vec![make_tool("latex_pdf__compile_latex", &["source"])];
        let err =
            validate_tool_args("latex_pdf__compile_latex", r#""just a string""#, &tools)
                .expect_err("expected type error");
        assert!(err.contains("object"));
    }

    #[test]
    fn validate_tool_args_unknown_tool_passes_through() {
        let tools: Vec<ToolDefinition> = Vec::new();
        let out = validate_tool_args("totally__unknown_tool", r#"{"x":1}"#, &tools);
        assert!(out.is_ok());
    }

    // ── required_fields_for ────────────────────────────────────────────────

    #[test]
    fn required_fields_for_returns_expected_names() {
        let tools = vec![make_tool(
            "latex_pdf__create_math_document",
            &["filename", "title", "sections"],
        )];
        let got =
            required_fields_for("latex_pdf__create_math_document", &tools).unwrap();
        assert_eq!(got, vec!["filename", "title", "sections"]);
    }

    #[test]
    fn required_fields_for_returns_none_for_missing_tool() {
        let tools: Vec<ToolDefinition> = Vec::new();
        assert!(required_fields_for("missing", &tools).is_none());
    }

    // ── collect_recent_user_text ───────────────────────────────────────────

    #[test]
    fn collect_recent_user_text_lowercases_and_limits() {
        // Oldest → newest. The helper takes the N MOST RECENT user messages
        // (iterator reversed internally), so only the last two user turns
        // must appear in the result.
        let messages = vec![
            ("user".to_string(), "IGNORED OLD".to_string()),
            ("assistant".to_string(), "sure".to_string()),
            ("user".to_string(), "Also ADD a Theorem".to_string()),
            ("assistant".to_string(), "ok".to_string()),
            ("user".to_string(), "Write LATEX doc".to_string()),
        ];
        let out = collect_recent_user_text(&messages, 2);
        assert!(out.contains("write latex doc"), "got: {out:?}");
        assert!(out.contains("also add a theorem"), "got: {out:?}");
        assert!(!out.contains("ignored old"), "got: {out:?}");
    }

    // ── last_assistant_tool_server ─────────────────────────────────────────

    #[test]
    fn last_assistant_tool_server_extracts_from_structured_assistant_message() {
        let asst = json!({
            "role": "assistant",
            "content": "calling...",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": "latex_pdf__compile_latex", "arguments": "{}" }
            }]
        })
        .to_string();
        let messages = vec![
            ("user".to_string(), "hello".to_string()),
            ("assistant".to_string(), asst),
        ];
        assert_eq!(
            last_assistant_tool_server(&messages),
            Some("latex_pdf".to_string())
        );
    }

    #[test]
    fn last_assistant_tool_server_returns_none_when_no_tool_calls() {
        let messages = vec![
            ("user".to_string(), "hi".to_string()),
            ("assistant".to_string(), "hello back".to_string()),
        ];
        assert!(last_assistant_tool_server(&messages).is_none());
    }

    // ── extract_error_from_result ──────────────────────────────────────────

    #[test]
    fn extract_error_returns_none_for_plain_text() {
        assert!(extract_error_from_result("not json").is_none());
        assert!(extract_error_from_result("").is_none());
    }

    #[test]
    fn extract_error_returns_none_for_success_payload() {
        let ok = json!({
            "status": "created",
            "path": "/tmp/doc.pdf",
            "pages": 3,
        })
        .to_string();
        assert!(extract_error_from_result(&ok).is_none());
    }

    #[test]
    fn extract_error_returns_string_body_for_error_payload() {
        let err = json!({
            "error": "LaTeX compilation failed",
            "engine": "tectonic.exe",
            "log_tail": "Bad math environment delimiter",
        })
        .to_string();
        let out = extract_error_from_result(&err).expect("detected");
        assert_eq!(out, "LaTeX compilation failed");
    }

    #[test]
    fn extract_error_stringifies_non_string_error_values() {
        let err = json!({
            "error": { "code": 42, "message": "nested" },
        })
        .to_string();
        let out = extract_error_from_result(&err).expect("detected");
        assert!(out.contains("nested"), "got: {out:?}");
        assert!(out.contains("42"), "got: {out:?}");
    }

    #[test]
    fn extract_error_truncates_long_messages() {
        let huge = "x".repeat(1000);
        let err = json!({ "error": huge }).to_string();
        let out = extract_error_from_result(&err).expect("detected");
        assert!(out.len() <= 400, "expected <=400 chars, got {}", out.len());
    }
}

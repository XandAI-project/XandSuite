use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events broadcast from the Tauri backend to HTTP/SSE clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiEvent {
    ChatToken {
        conversation_id: String,
        token: String,
        done: bool,
    },
    ChatThinking {
        conversation_id: String,
        token: String,
    },
    ChatToolCall {
        conversation_id: String,
        tool_call_id: String,
        function_name: String,
        arguments: Value,
        turn: u32,
    },
    ChatToolResult {
        conversation_id: String,
        tool_call_id: String,
        result: String,
    },
    ArtifactUpdated {
        conversation_id: String,
        artifact_id: String,
    },
    GalleryUpdated {
        conversation_id: String,
    },
    AppLog {
        level: String,
        message: String,
        ts: String,
    },
    DownloadProgress {
        model_id: String,
        filename: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        status: String,
    },
    AgentEvent {
        task_id: String,
        event_type: String,
        payload: Value,
    },
    ServerStatus {
        running: bool,
        model: Option<String>,
    },
    /// Emitted whenever the set of connected MCP servers changes (a package
    /// was installed, uninstalled, or reconfigured), so HTTP/SSE clients can
    /// re-fetch `/skills/servers` and `/skills/tools` just like the desktop
    /// skills store does on the Tauri `skills_updated` event.
    SkillsUpdated {
        ts: String,
    },
    /// Emitted when the backend discards accumulated `chat_thinking` content
    /// because it was promoted to the visible response body. Mirrors the
    /// desktop `chat_thinking_clear` Tauri event.
    ChatThinkingClear {
        conversation_id: String,
    },
}

/// Convert a Tauri event name + JSON payload to an ApiEvent if possible.
pub fn from_tauri_event(event_name: &str, payload: &Value) -> Option<ApiEvent> {
    match event_name {
        "chat_token" => Some(ApiEvent::ChatToken {
            conversation_id: payload["conversation_id"].as_str().unwrap_or("").to_string(),
            token: payload["token"].as_str().unwrap_or("").to_string(),
            done: payload["done"].as_bool().unwrap_or(false),
        }),
        "chat_thinking" => Some(ApiEvent::ChatThinking {
            conversation_id: payload["conversation_id"].as_str().unwrap_or("").to_string(),
            token: payload["token"].as_str().unwrap_or("").to_string(),
        }),
        "chat_tool_call" => Some(ApiEvent::ChatToolCall {
            conversation_id: payload["conversation_id"].as_str().unwrap_or("").to_string(),
            tool_call_id: payload["tool_call_id"].as_str().unwrap_or("").to_string(),
            function_name: payload["function_name"].as_str().unwrap_or("").to_string(),
            arguments: payload["arguments"].clone(),
            turn: payload["turn"].as_u64().unwrap_or(0) as u32,
        }),
        "chat_tool_result" => Some(ApiEvent::ChatToolResult {
            conversation_id: payload["conversation_id"].as_str().unwrap_or("").to_string(),
            tool_call_id: payload["tool_call_id"].as_str().unwrap_or("").to_string(),
            result: payload["result"].as_str().unwrap_or("").to_string(),
        }),
        "artifact_updated" => Some(ApiEvent::ArtifactUpdated {
            conversation_id: payload["conversation_id"].as_str().unwrap_or("").to_string(),
            artifact_id: payload["artifact_id"].as_str().unwrap_or("").to_string(),
        }),
        "gallery_updated" => Some(ApiEvent::GalleryUpdated {
            conversation_id: payload["conversation_id"].as_str().unwrap_or("").to_string(),
        }),
        "app_log" => Some(ApiEvent::AppLog {
            level: payload["level"].as_str().unwrap_or("info").to_string(),
            message: payload["message"].as_str().unwrap_or("").to_string(),
            ts: payload["ts"].as_str().unwrap_or("").to_string(),
        }),
        "download_progress" => Some(ApiEvent::DownloadProgress {
            model_id: payload["model_id"].as_str().unwrap_or("").to_string(),
            filename: payload["filename"].as_str().unwrap_or("").to_string(),
            downloaded_bytes: payload["downloaded_bytes"].as_u64().unwrap_or(0),
            total_bytes: payload["total_bytes"].as_u64(),
            status: payload["status"].as_str().unwrap_or("downloading").to_string(),
        }),
        "agent_event" => Some(ApiEvent::AgentEvent {
            task_id: payload["task_id"].as_str().unwrap_or("").to_string(),
            event_type: payload["event_type"].as_str().unwrap_or("").to_string(),
            payload: payload["payload"].clone(),
        }),
        "skills_updated" => Some(ApiEvent::SkillsUpdated {
            ts: payload["ts"].as_str().unwrap_or("").to_string(),
        }),
        "chat_thinking_clear" => Some(ApiEvent::ChatThinkingClear {
            conversation_id: payload["conversation_id"].as_str().unwrap_or("").to_string(),
        }),
        _ => None,
    }
}

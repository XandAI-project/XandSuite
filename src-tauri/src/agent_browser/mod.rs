//! Embedded browser agent.
//!
//! Drives a sidecar Chromium/Chrome instance through the Chrome DevTools
//! Protocol (via `chromiumoxide`) and exposes a set of `browser_agent__*`
//! tools to the existing `SkillsExecutor`. The viewport is streamed to the
//! frontend as base64 JPEG frames using `Page.startScreencast`; input is
//! forwarded back through `Input.dispatchMouseEvent` /
//! `Input.dispatchKeyEvent`.
//!
//! Integration touch points:
//!   - `src-tauri/src/skills/executor.rs`
//!     - dispatches `browser_agent__*` tool calls
//!     - scoping prefers browser tools when the conversation is in
//!       Browser Agent mode
//!   - `src-tauri/src/commands/chat.rs`
//!     - appends a conditional "Browser Agent" section to the system prompt
//!   - `src-tauri/src/state.rs` (future): holds the per-app
//!     `BrowserSessionRegistry` so the executor can reach the active
//!     controller for a given conversation.

pub mod controller;
pub mod cookie_vault;
pub mod events;
pub mod profile;
pub mod safety;
pub mod session;
pub mod snapshot;
pub mod tools;

#[allow(unused_imports)]
pub use controller::BrowserController;
#[allow(unused_imports)]
pub use events::{
    emit_confirm_request, emit_frame, emit_load_state, emit_title, emit_url,
    BrowserAgentFrame,
};
#[allow(unused_imports)]
pub use profile::{ProfileKind, ProfileManager};
#[allow(unused_imports)]
pub use safety::{SafetyAction, SafetyDecision, SafetyGate};
#[allow(unused_imports)]
pub use session::{BrowserSession, BrowserSessionRegistry, SessionId};
#[allow(unused_imports)]
pub use snapshot::{InteractiveNode, NodeLookup, SnapshotResult, SnapshotService};
#[allow(unused_imports)]
pub use tools::{
    browser_agent_tool_definitions, dispatch_browser_agent, BROWSER_AGENT_SERVER_ID,
};

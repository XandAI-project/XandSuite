//! Per-conversation browser agent session.
//!
//! Each session pairs a conversation id with a single `BrowserController`
//! and a `SnapshotService`. The registry lets the executor look up the
//! active controller for a conversation when dispatching a
//! `browser_agent__*` tool call.

use anyhow::Result;
use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex as TokioMutex;

use super::controller::BrowserController;
use super::cookie_vault::{self, CookieVault};
use super::snapshot::{NodeLookup, SnapshotService};

pub type SessionId = String;

pub struct BrowserSession {
    pub id: SessionId,
    pub conversation_id: String,
    pub controller: Arc<BrowserController>,
    pub snapshot: SnapshotService,
    pub node_lookup: Arc<NodeLookup>,
    /// Monotonic turn counter; incremented on every successful `snapshot`
    /// so the lookup table for the previous turn is invalidated.
    pub turn: std::sync::atomic::AtomicU32,
    /// Vault session ids whose cookies have already been pushed to the
    /// Chromium instance for this browser session. Used by
    /// `auto_inject_cookies` to avoid re-sending the same batch on every
    /// navigation. A plain `std::sync::Mutex` is fine here because the
    /// critical section is a couple of `HashSet` operations — never held
    /// across an `.await`.
    pub applied_cookie_sessions: StdMutex<HashSet<String>>,
}

impl BrowserSession {
    pub fn new(conversation_id: String, controller: Arc<BrowserController>) -> Arc<Self> {
        let lookup = NodeLookup::new();
        Arc::new(Self {
            id: uuid_like_id(),
            conversation_id,
            controller,
            snapshot: SnapshotService::new(lookup.clone()),
            node_lookup: lookup,
            turn: std::sync::atomic::AtomicU32::new(0),
            applied_cookie_sessions: StdMutex::new(HashSet::new()),
        })
    }

    pub fn current_turn(&self) -> u32 {
        self.turn.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn bump_turn(&self) -> u32 {
        self.turn
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    /// Record that `vault_session_id` has already been applied to this
    /// browser session. Called by `start_browser_session` after it
    /// manually applies the user-selected cookie bundle so that the
    /// follow-up auto-inject pass doesn't push the same cookies twice.
    pub fn mark_cookie_session_applied(&self, vault_session_id: &str) {
        if let Ok(mut g) = self.applied_cookie_sessions.lock() {
            g.insert(vault_session_id.to_string());
        }
    }

    /// Look up every vault session that matches `url`, skip the ones we
    /// have already applied to this browser, and push the rest to
    /// Chromium via `Network.setCookies`.
    ///
    /// Returns the number of cookies actually sent (summed across the new
    /// sessions). `0` means either no matching sessions exist or every
    /// match was already applied — both are silent no-ops for the caller.
    pub async fn auto_inject_cookies(
        &self,
        url: &str,
        vault: &CookieVault,
    ) -> Result<usize> {
        let candidates = vault.sessions_for_domain(url);
        if candidates.is_empty() {
            return Ok(0);
        }

        // Filter out already-applied ids without holding the lock across
        // the async set_cookies call.
        let new_sessions: Vec<_> = {
            let guard = match self.applied_cookie_sessions.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            candidates
                .into_iter()
                .filter(|s| !guard.contains(&s.id))
                .collect()
        };
        if new_sessions.is_empty() {
            return Ok(0);
        }

        let mut params = Vec::new();
        let mut applied_ids = Vec::with_capacity(new_sessions.len());
        for s in &new_sessions {
            params.extend(cookie_vault::to_cookie_params(&s.cookies));
            applied_ids.push(s.id.clone());
        }
        if params.is_empty() {
            // Nothing CDP-viable — still mark as "tried" so we don't retry
            // on every navigation.
            if let Ok(mut g) = self.applied_cookie_sessions.lock() {
                for id in applied_ids {
                    g.insert(id);
                }
            }
            return Ok(0);
        }

        let pushed = self.controller.set_cookies(params).await?;
        if let Ok(mut g) = self.applied_cookie_sessions.lock() {
            for id in applied_ids {
                g.insert(id);
            }
        }
        Ok(pushed)
    }
}

/// Per-app registry of active browser sessions, keyed by conversation id.
///
/// The `tokio::sync::Mutex` here is intentional: registry operations are
/// always awaited from async contexts (the executor's tool dispatch path,
/// Tauri command handlers) and the guard may outlive an `.await` when
/// starting a session.
pub struct BrowserSessionRegistry {
    sessions: TokioMutex<std::collections::HashMap<String, Arc<BrowserSession>>>,
}

impl Default for BrowserSessionRegistry {
    fn default() -> Self {
        Self {
            sessions: TokioMutex::new(std::collections::HashMap::new()),
        }
    }
}

impl BrowserSessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get(&self, conversation_id: &str) -> Option<Arc<BrowserSession>> {
        self.sessions.lock().await.get(conversation_id).cloned()
    }

    pub async fn insert(&self, session: Arc<BrowserSession>) {
        self.sessions
            .lock()
            .await
            .insert(session.conversation_id.clone(), session);
    }

    pub async fn remove(&self, conversation_id: &str) -> Option<Arc<BrowserSession>> {
        self.sessions.lock().await.remove(conversation_id)
    }

    pub async fn list_conversations(&self) -> Vec<String> {
        self.sessions.lock().await.keys().cloned().collect()
    }
}

/// Small helper to avoid a hard dependency on `uuid` just for session ids.
/// The `uuid` crate is already a workspace dep, so this is a thin wrapper.
fn uuid_like_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

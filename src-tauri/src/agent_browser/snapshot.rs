//! Accessibility-tree snapshot + Set-of-Mark screenshot support.
//!
//! `SnapshotService::ax_snapshot` returns a pruned, interactive-only slice of
//! the accessibility tree, with every node assigned a stable numeric `index`.
//! The real pixel geometry is stored **server-side** in `NodeLookup` keyed by
//! `(session_id, turn, index)`; the LLM only ever sees the index, which
//! prevents it from being tricked into clicking at arbitrary off-screen
//! coordinates.
//!
//! `SnapshotService::som_screenshot` injects numeric badges over the
//! interactive nodes, captures a screenshot, and reverts the overlay. The cap
//! on interactive nodes keeps very large pages (think Amazon search results)
//! inside the context budget.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Hard cap on the number of interactive nodes returned per snapshot.
/// Raised cautiously: Amazon search results can emit 2k+ interactive nodes,
/// but past ~120 the model's performance drops sharply.
pub const MAX_SOM_NODES: usize = 120;

/// Single interactive element returned to the model.
///
/// Pixel geometry is intentionally **omitted** from the serialised form —
/// it lives in `NodeLookup` and is never sent to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveNode {
    pub index: u32,
    pub role: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub editable: bool,
    /// Best-effort "frame path" for elements inside iframes; empty for the
    /// top document. Format: `["iframe#0", "iframe#2"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame_path: Vec<String>,
}

/// Full geometry for a node — server-side only.
#[derive(Debug, Clone)]
pub struct NodeGeometry {
    /// CSS pixel bounds relative to the layout viewport.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Center-of-node click coordinates (CSS px).
    pub click_x: f64,
    pub click_y: f64,
    /// DOM object id from `DOM.describeNode` / backend node id — used to
    /// resolve focus/scroll on the actual element instead of a pixel hit.
    pub backend_node_id: Option<i64>,
    /// Frame chain a controller must hop through before dispatching input.
    pub frame_path: Vec<String>,
}

/// Server-side lookup: `(session_id, turn) → { index → geometry }`.
///
/// The snapshot tool builds a fresh entry every time it's called; `click` /
/// `type` tools resolve the index against the most recent entry for the
/// session.
#[derive(Default)]
pub struct NodeLookup {
    inner: Mutex<HashMap<String, HashMap<u32, NodeGeometry>>>,
}

impl NodeLookup {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn key(session_id: &str, turn: u32) -> String {
        format!("{}:{}", session_id, turn)
    }

    pub fn store(
        &self,
        session_id: &str,
        turn: u32,
        entries: HashMap<u32, NodeGeometry>,
    ) {
        let mut guard = self.inner.lock().unwrap();
        guard.insert(Self::key(session_id, turn), entries);
    }

    pub fn resolve(&self, session_id: &str, turn: u32, index: u32) -> Option<NodeGeometry> {
        let guard = self.inner.lock().unwrap();
        guard
            .get(&Self::key(session_id, turn))
            .and_then(|entries| entries.get(&index).cloned())
    }

    /// Drop lookup tables for a session when it closes.
    pub fn forget_session(&self, session_id: &str) {
        let mut guard = self.inner.lock().unwrap();
        guard.retain(|k, _| !k.starts_with(&format!("{}:", session_id)));
    }
}

/// Return value of a snapshot tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResult {
    pub url: String,
    pub title: String,
    pub nodes: Vec<InteractiveNode>,
    /// `true` when the pruning cap dropped nodes. The model is instructed
    /// (via the system prompt) to scroll or query more specifically when
    /// this is set.
    pub truncated: bool,
    /// Optional base64 PNG of a Set-of-Mark-annotated screenshot. Only
    /// populated when `include_screenshot=true` is requested on the tool
    /// call; keeps vision tokens out of context on the happy path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot_png_base64: Option<String>,
}

/// Perception service. Stateless aside from the shared `NodeLookup`.
#[derive(Clone)]
pub struct SnapshotService {
    lookup: Arc<NodeLookup>,
}

impl SnapshotService {
    pub fn new(lookup: Arc<NodeLookup>) -> Self {
        Self { lookup }
    }

    pub fn lookup(&self) -> Arc<NodeLookup> {
        self.lookup.clone()
    }

    /// Prune a raw accessibility tree down to at most `MAX_SOM_NODES`
    /// interactive entries and assign stable numeric indices.
    ///
    /// Pruning rules:
    /// - Keep nodes with a role in the `INTERACTIVE_ROLES` table.
    /// - Drop hidden / off-screen nodes (`ignored=true`, `hidden=true`).
    /// - Collapse structurally identical siblings (same role + same
    ///   accessible name) to a single representative.
    /// - Preserve document order — the index directly reflects the DOM
    ///   tab order, so the model can reason about layout without needing
    ///   pixel coordinates.
    ///
    /// This function is deliberately **pure** over the input `raw_nodes`
    /// so it can be tested without a running Chromium.
    pub fn prune_and_index(raw_nodes: Vec<RawAxNode>) -> (Vec<InteractiveNode>, bool) {
        let mut out: Vec<InteractiveNode> = Vec::new();
        let mut seen_sig: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();

        for raw in raw_nodes.into_iter() {
            if raw.ignored || raw.hidden {
                continue;
            }
            if !is_interactive_role(&raw.role) {
                continue;
            }
            let name = raw.name.trim().to_string();
            if name.is_empty() && raw.role != "textbox" && raw.role != "searchbox" {
                continue;
            }

            let sig = (raw.role.clone(), name.clone());
            if seen_sig.contains(&sig) {
                continue;
            }
            seen_sig.insert(sig);

            if out.len() >= MAX_SOM_NODES {
                return (out, true);
            }

            out.push(InteractiveNode {
                index: out.len() as u32,
                role: raw.role,
                name: truncate_name(&name, 120),
                value: raw.value.filter(|v| !v.is_empty()).map(|v| truncate_name(&v, 120)),
                editable: raw.editable,
                frame_path: raw.frame_path,
            });
        }

        (out, false)
    }
}

/// Raw node as lifted from Chromium's `Accessibility.getFullAXTree`. Kept
/// deliberately minimal so the pruning logic can be unit-tested without
/// depending on `chromiumoxide`'s generated types.
#[derive(Debug, Clone)]
pub struct RawAxNode {
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub editable: bool,
    pub ignored: bool,
    pub hidden: bool,
    pub frame_path: Vec<String>,
}

/// Interactive accessibility roles — anything the agent could reasonably
/// want to click, focus, or read. Sourced from the ARIA 1.2 widget roles
/// plus a few common native controls Chromium reports.
const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "combobox",
    "checkbox",
    "radio",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "tab",
    "switch",
    "slider",
    "spinbutton",
    "listbox",
    "option",
    "treeitem",
    "gridcell",
    "columnheader",
    "rowheader",
];

fn is_interactive_role(role: &str) -> bool {
    let lc = role.to_lowercase();
    INTERACTIVE_ROLES.iter().any(|r| *r == lc)
}

fn truncate_name(s: &str, max: usize) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max {
        collapsed
    } else {
        let truncated: String = collapsed.chars().take(max).collect();
        format!("{}…", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(role: &str, name: &str) -> RawAxNode {
        RawAxNode {
            role: role.into(),
            name: name.into(),
            value: None,
            editable: role == "textbox" || role == "searchbox",
            ignored: false,
            hidden: false,
            frame_path: Vec::new(),
        }
    }

    #[test]
    fn prune_keeps_only_interactive_roles() {
        let raw = vec![
            n("button", "Submit"),
            n("heading", "Page title"),
            n("paragraph", "Some text"),
            n("link", "Home"),
            n("textbox", ""),
        ];
        let (out, truncated) = SnapshotService::prune_and_index(raw);
        assert!(!truncated);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].role, "button");
        assert_eq!(out[1].role, "link");
        assert_eq!(out[2].role, "textbox");
    }

    #[test]
    fn prune_assigns_sequential_indices_starting_at_zero() {
        let raw = vec![n("button", "A"), n("button", "B"), n("button", "C")];
        let (out, _) = SnapshotService::prune_and_index(raw);
        assert_eq!(out.iter().map(|n| n.index).collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn prune_collapses_identical_siblings() {
        let raw = vec![
            n("button", "Add to cart"),
            n("button", "Add to cart"),
            n("button", "Add to cart"),
            n("button", "Checkout"),
        ];
        let (out, _) = SnapshotService::prune_and_index(raw);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "Add to cart");
        assert_eq!(out[1].name, "Checkout");
    }

    #[test]
    fn prune_drops_hidden_and_ignored_nodes() {
        let raw = vec![
            RawAxNode {
                ignored: true,
                ..n("button", "Hidden A")
            },
            RawAxNode {
                hidden: true,
                ..n("button", "Hidden B")
            },
            n("button", "Visible"),
        ];
        let (out, _) = SnapshotService::prune_and_index(raw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Visible");
    }

    #[test]
    fn prune_enforces_max_som_nodes_cap() {
        let raw: Vec<RawAxNode> = (0..(MAX_SOM_NODES + 25))
            .map(|i| n("button", &format!("btn-{}", i)))
            .collect();
        let (out, truncated) = SnapshotService::prune_and_index(raw);
        assert_eq!(out.len(), MAX_SOM_NODES);
        assert!(truncated);
    }

    #[test]
    fn node_lookup_roundtrip() {
        let lookup = NodeLookup::new();
        let mut entries = HashMap::new();
        entries.insert(
            3,
            NodeGeometry {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 40.0,
                click_x: 60.0,
                click_y: 40.0,
                backend_node_id: Some(42),
                frame_path: Vec::new(),
            },
        );
        lookup.store("sess-a", 2, entries);

        let g = lookup.resolve("sess-a", 2, 3).expect("geometry should resolve");
        assert_eq!(g.backend_node_id, Some(42));
        assert!(lookup.resolve("sess-a", 2, 7).is_none());
        assert!(lookup.resolve("sess-b", 2, 3).is_none());

        lookup.forget_session("sess-a");
        assert!(lookup.resolve("sess-a", 2, 3).is_none());
    }

    #[test]
    fn truncate_name_collapses_whitespace_and_caps_length() {
        let s = "  hello    world   foo   bar  ";
        assert_eq!(truncate_name(s, 50), "hello world foo bar");

        let long = "x".repeat(200);
        let got = truncate_name(&long, 20);
        assert_eq!(got.chars().count(), 21); // 20 x's + ellipsis
        assert!(got.ends_with('…'));
    }
}

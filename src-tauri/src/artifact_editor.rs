/// Artifact editor: view, str_replace, insert, undo_edit, rename.
///
/// Provides targeted, token-efficient edits to existing artifacts so the LLM
/// can patch documents in place instead of re-emitting the full content.
/// Follows the proven Anthropic `text_editor` / search-replace pattern.

use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::db::AppDb;

const MAX_VIEW_BYTES: usize = 200_000;
const MAX_UNDO_HISTORY: usize = 5;

// ── Public result types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ViewResult {
    pub artifact_id: String,
    pub title: String,
    pub artifact_type: String,
    pub language: Option<String>,
    pub total_lines: usize,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EditResult {
    pub artifact_id: String,
    pub title: String,
    pub artifact_type: String,
    pub total_lines: usize,
    pub snippet: String,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

struct ArtifactRow {
    id: String,
    conversation_id: String,
    title: String,
    artifact_type: String,
    language: Option<String>,
    content: String,
}

fn fetch_artifact(db: &AppDb, id: &str) -> Result<ArtifactRow> {
    db.conn
        .query_row(
            "SELECT id, conversation_id, title, artifact_type, language, content \
             FROM artifacts WHERE id = ?1",
            params![id],
            |row| {
                Ok(ArtifactRow {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    title: row.get(2)?,
                    artifact_type: row.get(3)?,
                    language: row.get(4)?,
                    content: row.get(5)?,
                })
            },
        )
        .map_err(|_| anyhow::anyhow!("Artifact not found: {}", id))
}

/// For PDF artifacts, the content is JSON metadata. The editable body lives at
/// `.source.body` inside that JSON. Returns the editable text, the source
/// format, and the parsed JSON wrapper, or `None` when the artifact is not a PDF.
fn extract_pdf_source(art: &ArtifactRow) -> Option<(String, String, Value)> {
    if art.artifact_type != "pdf" {
        return None;
    }
    let parsed: Value = serde_json::from_str(&art.content).ok()?;
    let source = parsed.get("source")?;
    let body = source.get("body")?.as_str()?;
    let format = source.get("format")?.as_str()?.to_string();
    Some((body.to_string(), format, parsed))
}

/// Get the editable text content for any artifact type. For PDFs this is the
/// source body; for everything else it's the raw content field.
fn editable_content(art: &ArtifactRow) -> Result<String> {
    if art.artifact_type == "pdf" {
        match extract_pdf_source(art) {
            Some((body, format, _)) => {
                if format == "sections" {
                    bail!(
                        "RenderUnsupported: This PDF was created with a structured sections payload \
                         (format='sections'). The artifact editor cannot patch structured JSON \
                         sections. To modify this PDF, call the original generator tool \
                         (e.g. create_pdf_report / create_math_document) with the updated sections."
                    );
                }
                Ok(body)
            }
            None => bail!(
                "This PDF artifact has no editable source. It was created before the editor was \
                 available. Regenerate the PDF using the original tool to enable editing."
            ),
        }
    } else {
        Ok(art.content.clone())
    }
}

/// Apply an updated body back into the artifact content. For PDFs, patches the
/// JSON source.body and marks the rendered file as stale. For others, returns as-is.
fn apply_content(art: &ArtifactRow, new_body: &str) -> String {
    if let Some((_, _, mut wrapper)) = extract_pdf_source(art) {
        if let Some(source) = wrapper.get_mut("source") {
            source["body"] = json!(new_body);
        }
        wrapper["source_edited"] = json!(true);
        serde_json::to_string(&wrapper).unwrap_or_else(|_| new_body.to_string())
    } else {
        new_body.to_string()
    }
}

fn number_lines(text: &str) -> String {
    text.lines()
        .enumerate()
        .map(|(i, line)| format!("{}: {}", i + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

/// Snapshot the current state before mutation for undo support.
fn snapshot_for_undo(db: &AppDb, art: &ArtifactRow) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    db.conn.execute(
        "INSERT INTO artifact_edit_history (id, artifact_id, prev_title, prev_content, edited_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, art.id, art.title, art.content, now],
    )?;
    // Trim old entries beyond MAX_UNDO_HISTORY
    db.conn.execute(
        "DELETE FROM artifact_edit_history WHERE artifact_id = ?1 AND id NOT IN \
         (SELECT id FROM artifact_edit_history WHERE artifact_id = ?1 ORDER BY edited_at DESC LIMIT ?2)",
        params![art.id, MAX_UNDO_HISTORY as i64],
    )?;
    Ok(())
}

fn update_content(db: &AppDb, id: &str, content: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    db.conn.execute(
        "UPDATE artifacts SET content = ?1, updated_at = ?2 WHERE id = ?3",
        params![content, now, id],
    )?;
    Ok(())
}

fn make_snippet(text: &str, around: &str) -> String {
    if let Some(pos) = text.find(around) {
        let start = text[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let snippet_end = (pos + around.len() + 200).min(text.len());
        let end = text[..snippet_end]
            .rfind('\n')
            .unwrap_or(snippet_end);
        let end = end.max(pos + around.len());
        let snippet = &text[start..end.min(text.len())];
        number_lines(snippet)
    } else {
        let preview_end = text.len().min(500);
        number_lines(&text[..preview_end])
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// View artifact content with line numbers. Supports optional `[start, end]`
/// range (1-indexed, inclusive).
pub fn view(
    db: &Arc<Mutex<AppDb>>,
    artifact_id: &str,
    view_range: Option<(usize, usize)>,
) -> Result<ViewResult> {
    let db = db.lock().map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
    let art = fetch_artifact(&db, artifact_id)?;
    let body = editable_content(&art)?;

    if body.len() > MAX_VIEW_BYTES && view_range.is_none() {
        bail!(
            "Content is {} bytes (>{} limit). Use view_range to read a section, e.g. [1, 200].",
            body.len(),
            MAX_VIEW_BYTES
        );
    }

    let lines: Vec<&str> = body.lines().collect();
    let total = lines.len();

    let numbered = if let Some((start, end)) = view_range {
        if start == 0 || start > total {
            bail!("start ({}) is out of range. File has {} lines.", start, total);
        }
        let end = end.min(total);
        lines[(start - 1)..end]
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{}: {}", start + i, l))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        number_lines(&body)
    };

    Ok(ViewResult {
        artifact_id: art.id,
        title: art.title,
        artifact_type: art.artifact_type,
        language: art.language,
        total_lines: total,
        content: numbered,
    })
}

/// Replace an exact string match in the artifact content.
pub fn str_replace(
    db: &Arc<Mutex<AppDb>>,
    artifact_id: &str,
    old_str: &str,
    new_str: &str,
    replace_all: bool,
) -> Result<EditResult> {
    if old_str == new_str {
        bail!("old_str and new_str are identical — no change needed.");
    }
    if old_str.is_empty() {
        bail!("old_str must not be empty. Use insert to add new content.");
    }

    let db = db.lock().map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
    let art = fetch_artifact(&db, artifact_id)?;
    let body = editable_content(&art)?;
    let count = body.matches(old_str).count();

    if count == 0 {
        bail!(
            "NoMatch: old_str was not found in artifact '{}'. \
             Use artifact_editor__view to inspect the current content and retry with exact text.",
            art.title
        );
    }

    if count > 1 && !replace_all {
        bail!(
            "MultipleMatches: old_str was found {} times in artifact '{}'. \
             Provide a more specific old_str with surrounding context to match exactly once, \
             or set replace_all=true to replace all occurrences.",
            count,
            art.title
        );
    }

    snapshot_for_undo(&db, &art)?;

    let new_body = if replace_all {
        body.replace(old_str, new_str)
    } else {
        body.replacen(old_str, new_str, 1)
    };

    let new_content = apply_content(&art, &new_body);
    update_content(&db, &art.id, &new_content)?;

    let mut snippet = make_snippet(&new_body, new_str);
    if art.artifact_type == "pdf" {
        snippet.push_str(
            "\n\n[Source updated. The PDF file on disk is now stale. \
             Call the original generator tool to re-render if the user needs \
             the updated PDF file.]"
        );
    }
    Ok(EditResult {
        artifact_id: art.id,
        title: art.title,
        artifact_type: art.artifact_type,
        total_lines: line_count(&new_body),
        snippet,
    })
}

/// Insert text after a given line number (0 = top of file).
pub fn insert(
    db: &Arc<Mutex<AppDb>>,
    artifact_id: &str,
    insert_line: usize,
    new_str: &str,
) -> Result<EditResult> {
    if new_str.is_empty() {
        bail!("new_str must not be empty.");
    }

    let db = db.lock().map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
    let art = fetch_artifact(&db, artifact_id)?;
    let body = editable_content(&art)?;
    let mut lines: Vec<&str> = body.lines().collect();
    let total = lines.len();

    if insert_line > total {
        bail!(
            "InvalidLine: insert_line {} is beyond end of file ({} lines). \
             Use 0 to insert at the top, or {} to append at the end.",
            insert_line,
            total,
            total
        );
    }

    snapshot_for_undo(&db, &art)?;

    let new_lines: Vec<&str> = new_str.lines().collect();
    for (i, line) in new_lines.iter().enumerate() {
        lines.insert(insert_line + i, line);
    }

    let new_body = lines.join("\n");
    let new_content = apply_content(&art, &new_body);
    update_content(&db, &art.id, &new_content)?;

    let mut snippet = make_snippet(&new_body, new_str);
    if art.artifact_type == "pdf" {
        snippet.push_str(
            "\n\n[Source updated. The PDF file on disk is now stale. \
             Call the original generator tool to re-render if the user needs \
             the updated PDF file.]"
        );
    }
    Ok(EditResult {
        artifact_id: art.id,
        title: art.title,
        artifact_type: art.artifact_type,
        total_lines: line_count(&new_body),
        snippet,
    })
}

/// Undo the last edit, restoring the previous snapshot.
pub fn undo_edit(
    db: &Arc<Mutex<AppDb>>,
    artifact_id: &str,
) -> Result<EditResult> {
    let db = db.lock().map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;

    // Fetch the most recent history entry
    let (hist_id, prev_title, prev_content): (String, String, String) = db
        .conn
        .query_row(
            "SELECT id, prev_title, prev_content FROM artifact_edit_history \
             WHERE artifact_id = ?1 ORDER BY edited_at DESC LIMIT 1",
            params![artifact_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| anyhow::anyhow!("No undo history for artifact '{}'.", artifact_id))?;

    let now = Utc::now().to_rfc3339();
    db.conn.execute(
        "UPDATE artifacts SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![prev_title, prev_content, now, artifact_id],
    )?;

    // Remove the consumed history entry
    db.conn.execute(
        "DELETE FROM artifact_edit_history WHERE id = ?1",
        params![hist_id],
    )?;

    // Determine the body for the result
    let body_preview = if prev_content.len() > 500 {
        format!("{}...", &prev_content[..500])
    } else {
        prev_content.clone()
    };

    Ok(EditResult {
        artifact_id: artifact_id.to_string(),
        title: prev_title,
        artifact_type: String::new(),
        total_lines: line_count(&prev_content),
        snippet: format!("Restored previous version.\n{}", body_preview),
    })
}

/// Rename an artifact, rejecting title collisions within the same conversation.
pub fn rename(
    db: &Arc<Mutex<AppDb>>,
    artifact_id: &str,
    new_title: &str,
) -> Result<EditResult> {
    if new_title.trim().is_empty() {
        bail!("new_title must not be empty.");
    }

    let db = db.lock().map_err(|e| anyhow::anyhow!("DB lock: {}", e))?;
    let art = fetch_artifact(&db, artifact_id)?;

    if art.title == new_title {
        bail!("The artifact already has this title.");
    }

    // Check for title collision within the same conversation + type
    let collision: bool = db.conn.query_row(
        "SELECT COUNT(*) > 0 FROM artifacts \
         WHERE conversation_id = ?1 AND title = ?2 AND artifact_type = ?3 AND id != ?4",
        params![art.conversation_id, new_title, art.artifact_type, art.id],
        |row| row.get(0),
    )?;

    if collision {
        bail!(
            "TitleCollision: another {} artifact named '{}' already exists in this conversation.",
            art.artifact_type,
            new_title
        );
    }

    snapshot_for_undo(&db, &art)?;

    let now = Utc::now().to_rfc3339();
    db.conn.execute(
        "UPDATE artifacts SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_title, now, art.id],
    )?;

    let body = editable_content(&art).unwrap_or_default();
    Ok(EditResult {
        artifact_id: art.id,
        title: new_title.to_string(),
        artifact_type: art.artifact_type,
        total_lines: line_count(&body),
        snippet: format!("Renamed from '{}' to '{}'.", art.title, new_title),
    })
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::AppDb;
    use std::sync::{Arc, Mutex};

    fn test_db() -> Arc<Mutex<AppDb>> {
        let tmp = tempfile::tempdir().unwrap();
        let db = AppDb::open(&tmp.path().to_path_buf()).unwrap();
        Arc::new(Mutex::new(db))
    }

    fn seed_artifact(db: &Arc<Mutex<AppDb>>, content: &str) -> String {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let guard = db.lock().unwrap();
        guard.conn.execute(
            "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, 'test', ?2, ?2)",
            params![&id, &now],
        ).unwrap();
        guard.conn.execute(
            "INSERT INTO artifacts (id, conversation_id, title, artifact_type, content, created_at, updated_at) \
             VALUES (?1, ?1, 'Test Doc', 'markdown', ?2, ?3, ?3)",
            params![&id, content, &now],
        ).unwrap();
        id
    }

    #[test]
    fn view_returns_numbered_lines() {
        let db = test_db();
        let id = seed_artifact(&db, "line one\nline two\nline three");
        let result = view(&db, &id, None).unwrap();
        assert_eq!(result.total_lines, 3);
        assert!(result.content.contains("1: line one"));
        assert!(result.content.contains("3: line three"));
    }

    #[test]
    fn view_range_works() {
        let db = test_db();
        let id = seed_artifact(&db, "a\nb\nc\nd\ne");
        let result = view(&db, &id, Some((2, 4))).unwrap();
        assert!(result.content.contains("2: b"));
        assert!(result.content.contains("4: d"));
        assert!(!result.content.contains("1: a"));
    }

    #[test]
    fn str_replace_unique_match() {
        let db = test_db();
        let id = seed_artifact(&db, "Hello world\nGoodbye world");
        let result = str_replace(&db, &id, "Hello", "Hi", false).unwrap();
        assert!(result.snippet.contains("Hi"));

        let v = view(&db, &id, None).unwrap();
        assert!(v.content.contains("Hi world"));
        assert!(v.content.contains("Goodbye world"));
    }

    #[test]
    fn str_replace_no_match_errors() {
        let db = test_db();
        let id = seed_artifact(&db, "Hello world");
        let err = str_replace(&db, &id, "xyz_not_here", "abc", false).unwrap_err();
        assert!(err.to_string().contains("NoMatch"));
    }

    #[test]
    fn str_replace_multiple_match_errors_without_flag() {
        let db = test_db();
        let id = seed_artifact(&db, "foo bar foo baz foo");
        let err = str_replace(&db, &id, "foo", "qux", false).unwrap_err();
        assert!(err.to_string().contains("MultipleMatches"));
        assert!(err.to_string().contains("3"));
    }

    #[test]
    fn str_replace_all_works() {
        let db = test_db();
        let id = seed_artifact(&db, "foo bar foo baz foo");
        let _ = str_replace(&db, &id, "foo", "qux", true).unwrap();
        let v = view(&db, &id, None).unwrap();
        assert!(!v.content.contains("foo"));
        assert!(v.content.contains("qux"));
    }

    #[test]
    fn insert_at_top() {
        let db = test_db();
        let id = seed_artifact(&db, "line one\nline two");
        let _ = insert(&db, &id, 0, "header").unwrap();
        let v = view(&db, &id, None).unwrap();
        assert!(v.content.starts_with("1: header"));
    }

    #[test]
    fn insert_at_end() {
        let db = test_db();
        let id = seed_artifact(&db, "line one\nline two");
        let _ = insert(&db, &id, 2, "footer").unwrap();
        let v = view(&db, &id, None).unwrap();
        assert!(v.content.contains("3: footer"));
    }

    #[test]
    fn insert_invalid_line_errors() {
        let db = test_db();
        let id = seed_artifact(&db, "only one line");
        let err = insert(&db, &id, 5, "text").unwrap_err();
        assert!(err.to_string().contains("InvalidLine"));
    }

    #[test]
    fn undo_restores_previous() {
        let db = test_db();
        let id = seed_artifact(&db, "original content");
        let _ = str_replace(&db, &id, "original", "modified", false).unwrap();

        let v1 = view(&db, &id, None).unwrap();
        assert!(v1.content.contains("modified"));

        let _ = undo_edit(&db, &id).unwrap();
        let v2 = view(&db, &id, None).unwrap();
        assert!(v2.content.contains("original"));
    }

    #[test]
    fn undo_with_no_history_errors() {
        let db = test_db();
        let id = seed_artifact(&db, "content");
        let err = undo_edit(&db, &id).unwrap_err();
        assert!(err.to_string().contains("No undo history"));
    }

    #[test]
    fn rename_works() {
        let db = test_db();
        let id = seed_artifact(&db, "content");
        let result = rename(&db, &id, "New Title").unwrap();
        assert_eq!(result.title, "New Title");
    }

    #[test]
    fn rename_collision_errors() {
        let db = test_db();
        let id = seed_artifact(&db, "content");
        let now = Utc::now().to_rfc3339();
        let id2 = Uuid::new_v4().to_string();
        {
            let guard = db.lock().unwrap();
            guard.conn.execute(
                "INSERT INTO artifacts (id, conversation_id, title, artifact_type, content, created_at, updated_at) \
                 VALUES (?1, ?2, 'Taken Title', 'markdown', 'x', ?3, ?3)",
                params![&id2, &id, &now],
            ).unwrap();
        }
        let err = rename(&db, &id, "Taken Title").unwrap_err();
        assert!(err.to_string().contains("TitleCollision"));
    }
}

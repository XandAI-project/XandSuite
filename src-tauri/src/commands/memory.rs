use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::models::MEMORY_COLLECTION_ID;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    /// The extracted fact text.
    pub content: String,
    /// Conversation ID the fact was extracted from.
    pub source: String,
    pub created_at: String,
}

#[tauri::command]
pub fn list_memory_entries(state: State<'_, AppState>) -> Result<Vec<MemoryEntry>, String> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .conn
        .prepare(
            "SELECT id, content, source_file, created_at
             FROM rag_documents
             WHERE collection_id = ?1
             ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let entries = stmt
        .query_map(params![MEMORY_COLLECTION_ID], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                content: row.get(1)?,
                source: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(entries)
}

#[tauri::command]
pub async fn delete_memory_entry(
    entry_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let db = state.db.lock().unwrap();
        db.conn
            .execute(
                "DELETE FROM rag_documents WHERE id = ?1 AND collection_id = ?2",
                params![entry_id, MEMORY_COLLECTION_ID],
            )
            .map_err(|e| e.to_string())?;
    }

    // Remove from the vector store as well.
    let rag = state.rag.lock().await;
    if let Err(e) = rag.delete_document_chunks(&entry_id) {
        log::warn!("Failed to remove memory entry chunks from vector store: {}", e);
    }

    Ok(())
}

#[tauri::command]
pub async fn clear_memory_entries(state: State<'_, AppState>) -> Result<(), String> {
    // Collect all document IDs first so we can purge their vector chunks.
    let doc_ids: Vec<String> = {
        let db = state.db.lock().unwrap();
        let mut stmt = db
            .conn
            .prepare(
                "SELECT id FROM rag_documents WHERE collection_id = ?1",
            )
            .map_err(|e| e.to_string())?;

        let ids: Vec<String> = stmt
            .query_map(params![MEMORY_COLLECTION_ID], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        ids
    };

    {
        let db = state.db.lock().unwrap();
        db.conn
            .execute(
                "DELETE FROM rag_documents WHERE collection_id = ?1",
                params![MEMORY_COLLECTION_ID],
            )
            .map_err(|e| e.to_string())?;
    }

    let rag = state.rag.lock().await;
    for doc_id in &doc_ids {
        if let Err(e) = rag.delete_document_chunks(doc_id) {
            log::warn!("Failed to remove chunks for doc {}: {}", doc_id, e);
        }
    }

    Ok(())
}

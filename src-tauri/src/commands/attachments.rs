use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::rag::ingest::ingest_file;

/// Read any file from disk and return its raw bytes as a base64-encoded string.
/// Used by the frontend to embed local files (e.g. PDFs) as data URIs.
#[tauri::command]
pub async fn read_file_as_base64(path: String) -> Result<String, String> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read '{}': {}", path, e))?;
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentContent {
    pub filename: String,
    pub content: String,
    pub file_type: String,
    pub char_count: usize,
}

/// Read a file and return its content formatted for LLM injection.
/// Reuses the RAG ingest pipeline for PDF extraction, CSV formatting, etc.
#[tauri::command]
pub async fn read_attachment(file_path: String) -> Result<AttachmentContent, String> {
    let path = Path::new(&file_path);

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let file_type = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt")
        .to_lowercase();

    let chunks = ingest_file(path)
        .await
        .map_err(|e| format!("Failed to read attachment '{}': {}", filename, e))?;

    // Concatenate all content chunks (e.g. multiple CSV rows or PDF pages)
    let content = chunks
        .into_iter()
        .map(|(text, _meta)| text)
        .collect::<Vec<_>>()
        .join("\n\n");

    let char_count = content.len();

    Ok(AttachmentContent {
        filename,
        content,
        file_type,
        char_count,
    })
}

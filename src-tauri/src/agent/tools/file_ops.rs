use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;

pub struct FileOpsTool {
    workspace_dir: PathBuf,
}

impl FileOpsTool {
    pub fn new(workspace_dir: PathBuf) -> Self {
        Self { workspace_dir }
    }

    fn safe_path(&self, relative_path: &str) -> Result<PathBuf> {
        let joined = self.workspace_dir.join(relative_path);
        let canonical = joined
            .canonicalize()
            .unwrap_or(joined.clone());

        // Prevent path traversal outside workspace
        if !canonical.starts_with(&self.workspace_dir) {
            anyhow::bail!("Path traversal detected: {}", relative_path);
        }
        Ok(joined)
    }

    pub async fn read_file(&self, path: &str) -> Result<Value> {
        let full_path = self.safe_path(path)?;
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))?;

        Ok(serde_json::json!({
            "path": path,
            "content": content,
            "size_bytes": content.len()
        }))
    }

    pub async fn write_file(&self, path: &str, content: &str) -> Result<Value> {
        let full_path = self.safe_path(path)?;

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directories")?;
        }

        tokio::fs::write(&full_path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))?;

        Ok(serde_json::json!({
            "path": path,
            "bytes_written": content.len(),
            "success": true
        }))
    }

    pub async fn list_files(&self, dir: Option<&str>) -> Result<Value> {
        let target = match dir {
            Some(d) => self.safe_path(d)?,
            None => self.workspace_dir.clone(),
        };

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&target)
            .await
            .context("Failed to list directory")?;

        while let Some(entry) = read_dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().await?.is_dir();
            entries.push(serde_json::json!({
                "name": name,
                "is_directory": is_dir
            }));
        }

        Ok(serde_json::json!({
            "directory": dir.unwrap_or("."),
            "entries": entries
        }))
    }
}

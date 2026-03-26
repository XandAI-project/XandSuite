use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::Client;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::models::{DownloadProgress, DownloadStatus};

pub struct HfDownloader {
    client: Client,
    models_dir: PathBuf,
    api_token: Option<String>,
}

impl HfDownloader {
    pub fn new(models_dir: PathBuf, api_token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            models_dir,
            api_token,
        }
    }

    /// Download a GGUF file from a URL, streaming progress events.
    pub async fn download_model(
        &self,
        model_id: &str,
        filename: &str,
        url: &str,
        progress_tx: mpsc::Sender<DownloadProgress>,
    ) -> Result<PathBuf> {
        // Create the model directory
        let safe_model_id = model_id.replace('/', "_");
        let model_dir = self.models_dir.join(&safe_model_id);
        tokio::fs::create_dir_all(&model_dir)
            .await
            .context("Failed to create model directory")?;

        let dest_path = model_dir.join(filename);

        // Skip if already downloaded
        if dest_path.exists() {
            let _ = progress_tx.send(DownloadProgress {
                model_id: model_id.to_string(),
                filename: filename.to_string(),
                downloaded_bytes: tokio::fs::metadata(&dest_path).await?.len(),
                total_bytes: None,
                status: DownloadStatus::Completed,
            }).await;
            return Ok(dest_path);
        }

        let _ = progress_tx.send(DownloadProgress {
            model_id: model_id.to_string(),
            filename: filename.to_string(),
            downloaded_bytes: 0,
            total_bytes: None,
            status: DownloadStatus::Downloading,
        }).await;

        let mut req = self.client.get(url);
        if let Some(token) = &self.api_token {
            req = req.bearer_auth(token);
        }

        let response = req
            .send()
            .await
            .context("Failed to start model download")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Download failed with status {}: {}",
                response.status(),
                url
            );
        }

        let total_bytes = response.content_length();
        let mut downloaded: u64 = 0;

        let tmp_path = dest_path.with_extension("gguf.tmp");
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .context("Failed to create temp download file")?;

        let mut stream = response.bytes_stream();
        let mut last_progress = std::time::Instant::now();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("Download stream error")?;
            file.write_all(&bytes)
                .await
                .context("Failed to write download chunk")?;
            downloaded += bytes.len() as u64;

            // Throttle progress events to every 500ms
            if last_progress.elapsed().as_millis() >= 500 {
                last_progress = std::time::Instant::now();
                let _ = progress_tx.send(DownloadProgress {
                    model_id: model_id.to_string(),
                    filename: filename.to_string(),
                    downloaded_bytes: downloaded,
                    total_bytes,
                    status: DownloadStatus::Downloading,
                }).await;
            }
        }

        file.flush().await?;
        drop(file);

        // Rename tmp to final
        tokio::fs::rename(&tmp_path, &dest_path)
            .await
            .context("Failed to finalize downloaded file")?;

        let _ = progress_tx.send(DownloadProgress {
            model_id: model_id.to_string(),
            filename: filename.to_string(),
            downloaded_bytes: downloaded,
            total_bytes,
            status: DownloadStatus::Completed,
        }).await;

        Ok(dest_path)
    }

    pub fn get_model_path(&self, model_id: &str, filename: &str) -> PathBuf {
        let safe_id = model_id.replace('/', "_");
        self.models_dir.join(safe_id).join(filename)
    }

    pub fn is_downloaded(&self, model_id: &str, filename: &str) -> bool {
        self.get_model_path(model_id, filename).exists()
    }

    pub fn list_downloaded_models(&self) -> Vec<(String, String)> {
        let mut models = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.models_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let model_id = entry.file_name().to_string_lossy().to_string();
                    if let Ok(files) = std::fs::read_dir(entry.path()) {
                        for file in files.flatten() {
                            let name = file.file_name().to_string_lossy().to_string();
                            if name.ends_with(".gguf") {
                                models.push((model_id.clone(), name));
                            }
                        }
                    }
                }
            }
        }
        models
    }

    pub async fn delete_model(&self, model_id: &str, filename: &str) -> Result<()> {
        let path = self.get_model_path(model_id, filename);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .context("Failed to delete model file")?;
        }
        Ok(())
    }
}

//! Downloads hexgrad/Kokoro-82M into the app-local HuggingFace cache by
//! delegating to `kokoro_server.py --download --hf-home <path>`.
//!
//! Progress log lines are forwarded as `DownloadProgress` events so the UI
//! can show them live.

use anyhow::Result;
use tokio::sync::mpsc;

use crate::models::{DownloadProgress, DownloadStatus};

pub async fn download_kokoro_models(
    data_dir: &std::path::Path,
    device: &str,
    tx: mpsc::Sender<DownloadProgress>,
) -> Result<()> {
    let log_path = std::env::temp_dir().join("kokoro-download.log");

    let mut child = crate::tts::KokoroManager::spawn_download(data_dir, &log_path, device)?;

    let _ = tx
        .send(DownloadProgress {
            model_id: "kokoro".into(),
            filename: "hexgrad/Kokoro-82M".into(),
            downloaded_bytes: 0,
            total_bytes: None,
            status: DownloadStatus::Downloading,
        })
        .await;

    let mut last_byte: u64 = 0;
    loop {
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                // Flush any remaining log output
                if let Ok(contents) = std::fs::read_to_string(&log_path) {
                    let bytes = contents.len() as u64;
                    if bytes > last_byte {
                        for line in contents[last_byte as usize..].lines() {
                            let _ = tx
                                .send(DownloadProgress {
                                    model_id: "kokoro".into(),
                                    filename: line.to_string(),
                                    downloaded_bytes: 0,
                                    total_bytes: None,
                                    status: DownloadStatus::Downloading,
                                })
                                .await;
                        }
                    }
                }
                if exit_status.success() {
                    let _ = tx
                        .send(DownloadProgress {
                            model_id: "kokoro".into(),
                            filename: "Download complete".into(),
                            downloaded_bytes: 0,
                            total_bytes: None,
                            status: DownloadStatus::Completed,
                        })
                        .await;
                    return Ok(());
                } else {
                    anyhow::bail!(
                        "kokoro download process exited with status {}",
                        exit_status
                    );
                }
            }
            Ok(None) => {
                // Still running — tail the log
                if let Ok(contents) = std::fs::read_to_string(&log_path) {
                    let bytes = contents.len() as u64;
                    if bytes > last_byte {
                        for line in contents[last_byte as usize..].lines() {
                            let _ = tx
                                .send(DownloadProgress {
                                    model_id: "kokoro".into(),
                                    filename: line.to_string(),
                                    downloaded_bytes: 0,
                                    total_bytes: None,
                                    status: DownloadStatus::Downloading,
                                })
                                .await;
                        }
                        last_byte = bytes;
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => {
                anyhow::bail!("Failed to wait for download process: {}", e);
            }
        }
    }
}

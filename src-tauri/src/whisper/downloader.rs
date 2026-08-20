use anyhow::{Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

use crate::models::{DownloadProgress, DownloadStatus};

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Download the whisper-server binary from the latest whisper.cpp GitHub release.
/// Extracts all files to `bin_dir` (isolated from the llama-server DLLs).
/// `variant` should be `"cpu"` or `"cuda"`.
pub async fn download_whisper_binary(
    bin_dir: &Path,
    variant: &str,
    progress_tx: mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf> {
    // Remove the existing whisper directory so stale or locked DLLs from a
    // previous install do not block file creation on Windows.
    if bin_dir.exists() {
        tokio::fs::remove_dir_all(bin_dir)
            .await
            .context("Failed to remove existing whisper bin directory before re-download")?;
    }
    tokio::fs::create_dir_all(bin_dir).await?;

    let client = Client::builder()
        .user_agent("XandSuite/0.1 (github.com/xandnet/xandsuite)")
        .build()?;

    let release: GithubRelease = client
        .get("https://api.github.com/repos/ggml-org/whisper.cpp/releases/latest")
        .send()
        .await
        .context("Failed to fetch whisper.cpp release info")?
        .json()
        .await
        .context("Failed to parse whisper.cpp release JSON")?;

    let asset = find_asset(&release.assets, variant)
        .with_context(|| {
            format!(
                "No suitable whisper-server asset found in whisper.cpp release {}",
                release.tag_name
            )
        })?;

    log::info!(
        "Downloading whisper-server from {} ({} bytes)",
        asset.browser_download_url,
        asset.size
    );

    let _ = progress_tx
        .send(DownloadProgress {
            model_id: "whisper-server".into(),
            filename: asset.name.clone(),
            downloaded_bytes: 0,
            total_bytes: Some(asset.size),
            status: DownloadStatus::Downloading,
        })
        .await;

    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .context("Failed to start whisper-server download")?;

    if !response.status().is_success() {
        anyhow::bail!("Download returned status {}", response.status());
    }

    let total = response.content_length().unwrap_or(asset.size);
    let mut downloaded: u64 = 0;
    let mut archive_bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut stream = response.bytes_stream();
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("Download stream error")?;
        archive_bytes.extend_from_slice(&bytes);
        downloaded += bytes.len() as u64;

        if last_report.elapsed().as_millis() >= 300 {
            last_report = std::time::Instant::now();
            let _ = progress_tx
                .send(DownloadProgress {
                    model_id: "whisper-server".into(),
                    filename: asset.name.clone(),
                    downloaded_bytes: downloaded,
                    total_bytes: Some(total),
                    status: DownloadStatus::Downloading,
                })
                .await;
        }
    }

    let bin_name = if cfg!(target_os = "windows") {
        "whisper-server.exe"
    } else {
        "whisper-server"
    };

    if asset.name.ends_with(".zip") {
        extract_zip(&archive_bytes, bin_dir)?;
    } else {
        extract_targz(&archive_bytes, bin_dir)?;
    }

    let dest = bin_dir.join(bin_name);
    if !dest.exists() {
        anyhow::bail!(
            "'{}' was not found in the archive after extraction (extracted to {:?}). \
             The whisper.cpp release may package the binary under a different name.",
            bin_name, bin_dir
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(entries) = std::fs::read_dir(bin_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        let mut perms = meta.permissions();
                        perms.set_mode(0o755);
                        let _ = std::fs::set_permissions(entry.path(), perms);
                    }
                }
            }
        }
    }

    let _ = progress_tx
        .send(DownloadProgress {
            model_id: "whisper-server".into(),
            filename: asset.name.clone(),
            downloaded_bytes: downloaded,
            total_bytes: Some(total),
            status: DownloadStatus::Completed,
        })
        .await;

    log::info!("whisper-server extracted to {:?}", dest);
    Ok(dest)
}

/// Available Whisper model sizes.
pub const WHISPER_SIZES: &[&str] = &["tiny", "base", "small", "medium", "large-v3"];

/// Download a ggml Whisper model from Hugging Face.
/// Saves to `{models_dir}/ggml-{size}.bin`.
pub async fn download_whisper_model(
    size: &str,
    models_dir: &Path,
    progress_tx: mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf> {
    if !WHISPER_SIZES.contains(&size) {
        anyhow::bail!("Unknown whisper model size '{}'. Valid: {:?}", size, WHISPER_SIZES);
    }

    tokio::fs::create_dir_all(models_dir).await?;

    let filename = format!("ggml-{}.bin", size);
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        filename
    );
    let dest = models_dir.join(&filename);
    let model_id = format!("whisper-{}", size);

    let client = Client::builder()
        .user_agent("XandSuite/0.1")
        .build()?;

    log::info!("Downloading Whisper model {} from {}", size, url);

    let _ = progress_tx
        .send(DownloadProgress {
            model_id: model_id.clone(),
            filename: filename.clone(),
            downloaded_bytes: 0,
            total_bytes: None,
            status: DownloadStatus::Downloading,
        })
        .await;

    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to start Whisper model download")?;

    if !response.status().is_success() {
        anyhow::bail!("HuggingFace returned status {}", response.status());
    }

    let total = response.content_length();
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_report = std::time::Instant::now();

    let mut file = tokio::fs::File::create(&dest)
        .await
        .with_context(|| format!("Failed to create file {:?}", dest))?;

    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("Download stream error")?;
        file.write_all(&bytes).await.context("Failed to write model bytes")?;
        downloaded += bytes.len() as u64;

        if last_report.elapsed().as_millis() >= 300 {
            last_report = std::time::Instant::now();
            let _ = progress_tx
                .send(DownloadProgress {
                    model_id: model_id.clone(),
                    filename: filename.clone(),
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                    status: DownloadStatus::Downloading,
                })
                .await;
        }
    }

    file.flush().await?;

    let _ = progress_tx
        .send(DownloadProgress {
            model_id: model_id.clone(),
            filename: filename.clone(),
            downloaded_bytes: downloaded,
            total_bytes: total.or(Some(downloaded)),
            status: DownloadStatus::Completed,
        })
        .await;

    log::info!("Whisper model {} saved to {:?}", size, dest);
    Ok(dest)
}

// ── Asset selection ─────────────────────────────────────────────────────────

/// Select the best release asset for the requested variant.
///
/// whisper.cpp Windows asset names as of v1.7+:
///   whisper-bin-Win32.zip          — 32-bit CPU (skipped)
///   whisper-bin-x64.zip            — 64-bit CPU (minimal)
///   whisper-blas-bin-x64.zip       — 64-bit CPU + OpenBLAS (recommended CPU)
///   whisper-cublas-11.8.0-bin-x64.zip — CUDA 11
///   whisper-cublas-12.4.0-bin-x64.zip — CUDA 12 (RTX 20/30/40 series)
fn find_asset<'a>(assets: &'a [GithubAsset], variant: &str) -> Option<&'a GithubAsset> {
    // Ordered lists of substrings to try in preference order.
    // We use substring matching so minor version bumps in filenames still match.
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    let preferences: &[&str] = match variant {
        "cuda12" => &["cublas-12", "cublas-12.", "cublas-12.4"],
        "cuda11" => &["cublas-11", "cublas-11.", "cublas-11.8"],
        // Best CPU build first, then minimal CPU build as fallback
        _        => &["blas-bin-x64", "bin-x64"],
    };

    #[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
    let preferences: &[&str] = match variant {
        v if v.starts_with("cuda") => &["cublas", "cuda"],
        _ => &["blas-bin-x64", "bin-x64", "blas-bin-arm64", "bin-arm64"],
    };

    let is_zip = cfg!(target_os = "windows");
    let ext = if is_zip { ".zip" } else { ".tar.gz" };

    for pref in preferences {
        if let Some(a) = assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            n.ends_with(ext)
                && n.contains(pref)
                // Never select 32-bit builds
                && !n.contains("win32")
                // Skip framework/jar bundles
                && !n.contains("xcframework")
                && !n.contains(".jar")
        }) {
            return Some(a);
        }
    }

    log::warn!(
        "No whisper-server asset matched variant '{}'. Assets: {:?}",
        variant,
        assets.iter().map(|a| &a.name).collect::<Vec<_>>()
    );
    None
}

// ── Archive extraction ───────────────────────────────────────────────────────

fn extract_zip(data: &[u8], dest_dir: &Path) -> Result<()> {
    use std::io::Cursor;
    let cursor = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).context("Invalid zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let raw_path = entry.name().to_string();

        // Strip a single top-level directory if present
        let relative = strip_top_dir(&raw_path);
        if relative.is_empty() || entry.is_dir() {
            continue;
        }

        let out_path = dest_dir.join(relative);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)
            .with_context(|| format!("Failed to create {:?}", out_path))?;
        std::io::copy(&mut entry, &mut out)?;
    }

    Ok(())
}

fn extract_targz(data: &[u8], dest_dir: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Cursor;
    use tar::Archive;

    let cursor = Cursor::new(data);
    let gz = GzDecoder::new(cursor);
    let mut archive = Archive::new(gz);

    for entry in archive.entries().context("Failed to read tar archive")? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_string_lossy().to_string();
        let relative = strip_top_dir(&raw_path).to_string();
        if relative.is_empty() {
            continue;
        }
        let out_path = dest_dir.join(&relative);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Symlink entries (whisper.cpp's shared-library archives ship
        // versioned .so symlinks, e.g. libwhisper.so -> libwhisper.so.1) carry
        // zero data bytes. `entry.unpack()` was writing these as empty/garbage
        // regular files instead of real symlinks, so the dynamic linker could
        // never resolve `libwhisper.so` to the versioned library at runtime.
        // Mirror the working logic in server/downloader.rs::extract_targz.
        #[cfg(unix)]
        if entry.header().entry_type() == tar::EntryType::Symlink {
            if let Ok(Some(link_target)) = entry.link_name() {
                if out_path.symlink_metadata().is_ok() {
                    let _ = std::fs::remove_file(&out_path);
                }
                std::os::unix::fs::symlink(&link_target, &out_path).with_context(|| {
                    format!("Failed to create symlink {:?} -> {:?}", out_path, link_target)
                })?;
                continue;
            }
        }

        entry.unpack(&out_path)?;
    }

    Ok(())
}

fn strip_top_dir(path: &str) -> &str {
    if let Some(pos) = path.find('/') {
        let rest = &path[pos + 1..];
        if !rest.is_empty() {
            return rest;
        }
    }
    path
}

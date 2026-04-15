use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tar::Archive;
use tokio::sync::mpsc;

use crate::models::{DownloadProgress, DownloadStatus};

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryVariant {
    CpuOnly,
    Cuda12,
    /// CUDA 13.x — required for Blackwell GPUs (RTX 5000 series, GB2xx)
    Cuda13,
    Vulkan,
}

impl BinaryVariant {
    pub fn from_str(s: &str) -> Self {
        match s {
            "cuda12" => Self::Cuda12,
            "cuda13" => Self::Cuda13,
            "vulkan" => Self::Vulkan,
            _ => Self::CpuOnly,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::CpuOnly => "CPU only",
            Self::Cuda12 => "CUDA 12",
            Self::Cuda13 => "CUDA 13 (RTX 5000+)",
            Self::Vulkan => "Vulkan",
        }
    }
}

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

/// Download the llama-server binary from the latest llama.cpp GitHub release.
/// Emits `DownloadProgress` events via `progress_tx`.
/// Returns the path to the extracted binary.
pub async fn download_llama_server(
    variant: BinaryVariant,
    bin_dir: &Path,
    progress_tx: mpsc::Sender<DownloadProgress>,
) -> Result<PathBuf> {
    tokio::fs::create_dir_all(bin_dir).await?;

    let client = Client::builder()
        .user_agent("XandSuite/0.1 (github.com/xandnet/xandsuite)")
        .build()?;

    // Fetch latest release metadata (repo moved to ggml-org in 2025)
    let release: GithubRelease = client
        .get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest")
        .send()
        .await
        .context("Failed to fetch llama.cpp release info")?
        .json()
        .await
        .context("Failed to parse release JSON")?;

    let asset = find_asset(&release.assets, &variant)
        .with_context(|| {
            format!(
                "No {} asset found in llama.cpp release {}",
                variant.label(),
                release.tag_name
            )
        })?;

    log::info!(
        "Downloading llama-server {} from {} ({} bytes)",
        variant.label(),
        asset.browser_download_url,
        asset.size
    );

    // Send initial progress
    let _ = progress_tx
        .send(DownloadProgress {
            model_id: "llama-server".into(),
            filename: asset.name.clone(),
            downloaded_bytes: 0,
            total_bytes: Some(asset.size),
            status: DownloadStatus::Downloading,
        })
        .await;

    // Stream the zip download
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .context("Failed to start download")?;

    if !response.status().is_success() {
        anyhow::bail!("Download returned status {}", response.status());
    }

    let total = response.content_length().unwrap_or(asset.size);
    let mut downloaded: u64 = 0;
    let mut zip_bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut stream = response.bytes_stream();
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("Download stream error")?;
        zip_bytes.extend_from_slice(&bytes);
        downloaded += bytes.len() as u64;

        if last_report.elapsed().as_millis() >= 300 {
            last_report = std::time::Instant::now();
            let _ = progress_tx
                .send(DownloadProgress {
                    model_id: "llama-server".into(),
                    filename: asset.name.clone(),
                    downloaded_bytes: downloaded,
                    total_bytes: Some(total),
                    status: DownloadStatus::Downloading,
                })
                .await;
        }
    }

    // Extract all files from the archive into bin_dir.
    // llama.cpp b3000+ uses plugin DLLs (ggml-cpu.dll, ggml-cuda.dll, …)
    // that must sit alongside llama-server.exe — extracting only the binary
    // leaves those behind and causes "no backends are loaded" crashes.
    let bin_name = if cfg!(target_os = "windows") {
        "llama-server.exe"
    } else {
        "llama-server"
    };

    if asset.name.ends_with(".zip") {
        extract_all_from_zip(&zip_bytes, bin_dir)?;
    } else {
        extract_all_from_targz(&zip_bytes, bin_dir)?;
    }

    let dest = bin_dir.join(bin_name);
    if !dest.exists() {
        anyhow::bail!(
            "'{}' was not found in the archive after extraction. \
             Files were extracted to {:?}.",
            bin_name, bin_dir
        );
    }

    // Make all extracted files executable on Unix
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
            model_id: "llama-server".into(),
            filename: asset.name.clone(),
            downloaded_bytes: downloaded,
            total_bytes: Some(total),
            status: DownloadStatus::Completed,
        })
        .await;

    log::info!("llama-server extracted to {:?}", bin_dir);
    Ok(dest)
}

fn find_asset<'a>(assets: &'a [GithubAsset], variant: &BinaryVariant) -> Option<&'a GithubAsset> {
    let is_windows = cfg!(target_os = "windows");
    let is_linux = cfg!(target_os = "linux");

    let allowed_ext: &[&str] = if is_windows {
        &[".zip"]
    } else {
        &[".tar.gz", ".tar.xz"]
    };

    // Collect all candidates first so we can prefer x64 over arm64
    let candidates: Vec<&GithubAsset> = assets
        .iter()
        .filter(|a| {
            let name = a.name.to_lowercase();

            // Must be a binary package — exclude CUDA runtime-only zips like
            // "cudart-llama-bin-win-cuda-12.4-x64.zip" which contain DLLs only.
            if name.starts_with("cudart-") {
                return false;
            }

            // Extension check
            if !allowed_ext.iter().any(|ext| name.ends_with(ext)) {
                return false;
            }

            // Platform check
            let platform_ok = if is_windows {
                name.contains("-win-")
            } else if is_linux {
                name.contains("-ubuntu-") || name.contains("-linux-")
            } else {
                false
            };
            if !platform_ok {
                return false;
            }

            // Skip arm64/aarch64 on standard x86_64 hosts
            if name.contains("arm64") || name.contains("aarch64") {
                return false;
            }

            // Variant check — actual asset patterns in ggml-org/llama.cpp releases:
            //   Windows CPU:    llama-bXXXX-bin-win-cpu-x64.zip
            //   Linux CPU:      llama-bXXXX-bin-ubuntu-x64.tar.gz (no -cpu- suffix)
            //   CUDA12:         llama-bXXXX-bin-win-cuda-12.X-x64.zip
            //   Vulkan:         llama-bXXXX-bin-win-vulkan-x64.zip / llama-bXXXX-bin-ubuntu-vulkan-x64.tar.gz
            match variant {
                BinaryVariant::CpuOnly => {
                    // Windows: explicit -cpu- suffix
                    // Linux: no accelerator suffix (plain -ubuntu- or -linux-)
                    if is_windows {
                        name.contains("-cpu-")
                            && !name.contains("cuda")
                            && !name.contains("vulkan")
                    } else {
                        // Linux CPU builds don't have -cpu- suffix; exclude all accelerator types
                        !name.contains("cuda")
                            && !name.contains("vulkan")
                            && !name.contains("rocm")
                            && !name.contains("openvino")
                            && !name.contains("sycl")
                            && !name.contains("hip")
                    }
                }
                BinaryVariant::Cuda12 => {
                    (name.contains("cuda-12") || (name.contains("cuda") && name.contains("-12.")))
                        && !name.contains("-13.")
                }
                BinaryVariant::Cuda13 => {
                    name.contains("cuda-13") || (name.contains("cuda") && name.contains("-13."))
                }
                BinaryVariant::Vulkan => name.contains("vulkan"),
            }
        })
        .collect();

    // Prefer x64 explicitly; fall back to first match
    candidates
        .iter()
        .find(|a| a.name.to_lowercase().contains("x64"))
        .or_else(|| candidates.first())
        .copied()
}

/// Extract a zip archive into `dest_dir`, stripping the single top-level
/// directory prefix (e.g. `llama-b8429-bin-win-cuda-12.4-x64/`) but
/// preserving all subdirectories beneath it (e.g. `ggml-backends/`).
///
/// llama.cpp b3000+ ships backend plugins in `ggml-backends/` — flattening
/// that subdirectory causes "no backends are loaded" at runtime.
fn extract_all_from_zip(zip_bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to open zip archive")?;

    // Detect the single top-level directory prefix to strip, if present.
    // A top-level dir entry looks like "llama-b8429-bin-win-cpu-x64/" with no
    // further slashes before the trailing slash.
    let strip_prefix: Option<String> = {
        let mut prefix = None;
        for i in 0..archive.len() {
            if let Ok(f) = archive.by_index(i) {
                let name = f.name();
                // A root-level directory: exactly one '/' and it is at the end
                if name.ends_with('/') && name[..name.len() - 1].find('/').is_none() {
                    prefix = Some(name.to_string());
                    break;
                }
            }
        }
        prefix
    };

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let full_name = file.name().to_string();

        // Strip top-level prefix
        let relative = match &strip_prefix {
            Some(p) => full_name.strip_prefix(p.as_str()).unwrap_or(&full_name),
            None => &full_name,
        };

        // Skip directory entries and empty paths
        if relative.is_empty() || relative.ends_with('/') || relative.ends_with('\\') {
            continue;
        }

        let dest_path = dest_dir.join(relative);

        // Preserve subdirectories (e.g. ggml-backends/)
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {:?}", parent))?;
        }

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        create_file_replacing(&dest_path, &buf)?;

        log::debug!("extracted: {}", relative);
    }

    Ok(())
}

/// Extract a tar.gz archive into `dest_dir`, stripping the single top-level
/// directory prefix but preserving subdirectories beneath it.
fn extract_all_from_targz(gz_bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(gz_bytes);
    let gz = GzDecoder::new(cursor);
    let mut archive = Archive::new(gz);

    // First pass: detect the top-level prefix
    let strip_prefix: Option<String> = {
        let cursor2 = std::io::Cursor::new(gz_bytes);
        let gz2 = GzDecoder::new(cursor2);
        let mut archive2 = Archive::new(gz2);
        let mut prefix = None;
        for entry in archive2.entries()? {
            let entry = entry?;
            let path = entry.path()?;
            let components: Vec<_> = path.components().collect();
            if components.len() == 1 {
                prefix = Some(format!("{}/", components[0].as_os_str().to_string_lossy()));
                break;
            }
        }
        prefix
    };

    for entry in archive.entries().context("Failed to read tar archive")? {
        let mut entry = entry?;
        let full_path = entry.path()?.to_string_lossy().replace('\\', "/");

        let relative = match &strip_prefix {
            Some(p) => full_path.strip_prefix(p.as_str()).unwrap_or(&full_path).to_string(),
            None => full_path,
        };

        if relative.is_empty() || relative.ends_with('/') {
            continue;
        }

        let dest_path = dest_dir.join(&relative);
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        create_file_replacing(&dest_path, &buf)?;
    }

    Ok(())
}

/// Write `data` to `path`, creating or overwriting the file.
///
/// On Windows a DLL that was previously loaded (and since unloaded) may still
/// be "pending delete" or held by antivirus.  When `File::create` fails we
/// remove the existing file first and then retry once, which succeeds in the
/// common case where the old binary is no longer mapped into any process.
fn create_file_replacing(path: &Path, data: &[u8]) -> Result<()> {
    match std::fs::File::create(path) {
        Ok(mut f) => {
            f.write_all(data)
                .with_context(|| format!("Failed to write {:?}", path))?;
        }
        Err(_) if path.exists() => {
            // Remove the old copy and retry — handles the Windows "file in use"
            // case where the DLL cannot be opened for writing but can be deleted.
            std::fs::remove_file(path)
                .with_context(|| format!("Failed to remove locked file {:?}", path))?;
            std::fs::File::create(path)
                .with_context(|| format!("Failed to create {:?} after removing old copy", path))?
                .write_all(data)
                .with_context(|| format!("Failed to write {:?}", path))?;
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to create {:?}", path));
        }
    }
    Ok(())
}

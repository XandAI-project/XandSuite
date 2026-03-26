use std::path::PathBuf;
use std::time::Duration;

use crate::hf::scraper::HfScraper;

/// Background job that periodically syncs the HuggingFace GGUF model catalog.
/// Runs once on startup and then every 24 hours.
pub async fn run_hf_sync_loop(cache_dir: PathBuf, api_token: Option<String>) {
    log::info!("HF sync job started");

    loop {
        match sync_once(&cache_dir, &api_token).await {
            Ok(count) => log::info!("HF sync: updated {} models", count),
            Err(e) => log::warn!("HF sync failed: {}", e),
        }

        // Sleep 24 hours before next sync
        tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
    }
}

async fn sync_once(cache_dir: &PathBuf, api_token: &Option<String>) -> anyhow::Result<usize> {
    let scraper = HfScraper::new(api_token.clone());
    let models = scraper.fetch_gguf_models(100, None).await?;
    let count = models.len();
    scraper.save_cache(cache_dir, &models).await?;
    Ok(count)
}

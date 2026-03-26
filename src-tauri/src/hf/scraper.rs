use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::models::{GgufFile, HfModel};

const HF_API_BASE: &str = "https://huggingface.co/api";
const MODELS_CACHE_FILE: &str = "hf_models_cache.json";

#[derive(Debug, Deserialize)]
struct HfApiModel {
    id: String,
    author: Option<String>,
    #[serde(rename = "modelId")]
    model_id: Option<String>,
    downloads: Option<u64>,
    likes: Option<u64>,
    tags: Option<Vec<String>>,
    #[serde(rename = "lastModified")]
    last_modified: Option<String>,
    siblings: Option<Vec<ModelSibling>>,
    #[serde(rename = "cardData")]
    card_data: Option<CardData>,
}

#[derive(Debug, Deserialize)]
struct ModelSibling {
    rfilename: String,
    /// LFS metadata present for large files (includes the actual byte size)
    lfs: Option<LfsInfo>,
    /// Direct size field (non-LFS files)
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LfsInfo {
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CardData {
    language: Option<Vec<String>>,
    license: Option<String>,
    #[serde(rename = "model-index")]
    model_index: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelCache {
    pub models: Vec<HfModel>,
    pub last_updated: String,
}

pub struct HfScraper {
    client: Client,
    api_token: Option<String>,
}

impl HfScraper {
    pub fn new(api_token: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_token,
        }
    }

    /// Fetch available GGUF models from HuggingFace Hub API
    pub async fn fetch_gguf_models(
        &self,
        limit: u32,
        search: Option<&str>,
    ) -> Result<Vec<HfModel>> {
        // full=true includes `siblings` (file list) in the response
        let mut url = format!(
            "{}/models?filter=gguf&sort=downloads&direction=-1&limit={}&full=true",
            HF_API_BASE, limit
        );

        if let Some(q) = search {
            url.push_str(&format!("&search={}", urlencoding::encode(q)));
        }

        let mut req = self.client.get(&url);
        if let Some(token) = &self.api_token {
            req = req.bearer_auth(token);
        }

        let response = req
            .send()
            .await
            .context("Failed to connect to HuggingFace API")?;

        if !response.status().is_success() {
            let status = response.status();
            anyhow::bail!("HuggingFace API returned status {}", status);
        }

        let api_models: Vec<HfApiModel> = response
            .json()
            .await
            .context("Failed to parse HuggingFace API response")?;

        let models = api_models
            .into_iter()
            .map(|m| self.convert_model(m))
            .collect();

        Ok(models)
    }

    fn convert_model(&self, m: HfApiModel) -> HfModel {
        let author = m.author.clone().unwrap_or_else(|| {
            m.id.split('/').next().unwrap_or("unknown").to_string()
        });

        let gguf_files: Vec<GgufFile> = m
            .siblings
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|s| s.rfilename.ends_with(".gguf"))
            .map(|s| {
                let quantization = extract_quantization(&s.rfilename);
                // LFS size takes priority (actual file size), fallback to direct size field
                let size_bytes = s.lfs.as_ref().and_then(|l| l.size)
                    .or(s.size);
                GgufFile {
                    filename: s.rfilename.clone(),
                    size_bytes,
                    quantization,
                    url: format!(
                        "https://huggingface.co/{}/resolve/main/{}",
                        m.id, s.rfilename
                    ),
                }
            })
            .collect();

        HfModel {
            id: m.id.clone(),
            name: m.model_id.unwrap_or(m.id.clone()),
            author,
            description: None,
            tags: m.tags.unwrap_or_default(),
            downloads: m.downloads,
            likes: m.likes,
            last_modified: m.last_modified,
            gguf_files,
            is_downloaded: false,
            local_path: None,
        }
    }

    pub async fn save_cache(&self, cache_dir: &PathBuf, models: &[HfModel]) -> Result<()> {
        let cache = ModelCache {
            models: models.to_vec(),
            last_updated: chrono::Utc::now().to_rfc3339(),
        };
        let path = cache_dir.join(MODELS_CACHE_FILE);
        let json = serde_json::to_string_pretty(&cache)?;
        tokio::fs::write(&path, json)
            .await
            .context("Failed to write model cache")?;
        Ok(())
    }

    pub async fn load_cache(&self, cache_dir: &PathBuf) -> Result<Option<ModelCache>> {
        let path = cache_dir.join(MODELS_CACHE_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let json = tokio::fs::read_to_string(&path)
            .await
            .context("Failed to read model cache")?;
        let cache: ModelCache = serde_json::from_str(&json)
            .context("Failed to parse model cache")?;
        Ok(Some(cache))
    }
}

fn extract_quantization(filename: &str) -> Option<String> {
    let name = filename.to_uppercase();
    for quant in &[
        "Q8_0", "Q6_K", "Q5_K_M", "Q5_K_S", "Q5_0", "Q4_K_M", "Q4_K_S",
        "Q4_0", "Q3_K_L", "Q3_K_M", "Q3_K_S", "Q2_K", "IQ4_XS", "IQ3_M",
        "F16", "F32", "BF16",
    ] {
        if name.contains(quant) {
            return Some(quant.to_string());
        }
    }
    None
}

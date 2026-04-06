use anyhow::{Context, Result};
use serde::Deserialize;
use std::sync::{Arc, Mutex};

use crate::models::AppSettings;

/// Default embedding dimension used for zero-vector fallbacks.
/// 768 covers nomic-embed-text-v1.5 and most BGE models; the real
/// dimension is whatever llama-server returns for the loaded model.
pub const DEFAULT_DIM: usize = 768;

/// Generates embeddings by calling the running llama-server (or any
/// OpenAI-compatible server) at `POST /v1/embeddings`.
///
/// Zero external deps — uses the `reqwest` client already in the project.
/// If the server is not running the methods return gracefully degraded
/// zero vectors so ingestion/search still work (just less semantically accurate).
pub struct Embedder {
    client: reqwest::Client,
    /// Live reference to settings so the server URL is always up-to-date.
    settings: Arc<Mutex<AppSettings>>,
    /// Fallback zero-vector length (updated on first successful call).
    pub dim: usize,
}

impl Embedder {
    pub fn new(settings: Arc<Mutex<AppSettings>>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            settings,
            dim: DEFAULT_DIM,
        }
    }

    /// Embed a single text string.
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed_batch(&[text]).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Server returned no embeddings"))
    }

    /// Embed a batch of texts, returning one vector per input.
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let (url, model) = self.server_params();
        let endpoint = format!("{}/v1/embeddings", url.trim_end_matches('/'));

        let body = serde_json::json!({
            "model": model,
            "input": texts,
        });

        let resp = self
            .client
            .post(&endpoint)
            .json(&body)
            .send()
            .await
            .context("Failed to reach embedding server")?
            .error_for_status()
            .context("Embedding server returned error status")?;

        let parsed: EmbeddingResponse = resp
            .json()
            .await
            .context("Failed to parse embedding response")?;

        // Sort by index so the output order matches input order
        let mut data = parsed.data;
        data.sort_by_key(|e| e.index);

        let mut embeddings: Vec<Vec<f32>> = data.into_iter().map(|e| e.embedding).collect();

        // Normalise each vector in-place for cosine similarity
        for emb in embeddings.iter_mut() {
            normalize(emb);
        }

        Ok(embeddings)
    }

    /// Return (server_base_url, model_name) from current settings.
    fn server_params(&self) -> (String, String) {
        let s = self.settings.lock().unwrap();
        let url = if s.default_engine_mode == "remote" {
            s.remote_server_url
                .clone()
                .unwrap_or_else(|| format!("http://127.0.0.1:{}", s.llama_server_port))
        } else {
            format!("http://127.0.0.1:{}", s.llama_server_port)
        };
        // embedding_model is used as the model field in the request.
        // llama-server ignores it (uses whatever is loaded), but Ollama
        // and other OpenAI-compatible servers use it to route the request.
        let model = s.embedding_model.clone();
        (url, model)
    }
}

// ── OpenAI /v1/embeddings response types ─────────────────────────────────────

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingObject>,
}

#[derive(Deserialize)]
struct EmbeddingObject {
    embedding: Vec<f32>,
    index: usize,
}

// ── Math helpers ──────────────────────────────────────────────────────────────

/// L2-normalise a vector in place.
pub fn normalize(v: &mut Vec<f32>) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Cosine similarity between two (pre-normalised) vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

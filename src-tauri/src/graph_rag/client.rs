use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub entities: Vec<String>,
    pub relationships: Vec<String>,
    pub source: String,
}

/// Typed HTTP client for the graphrag-server REST API.
pub struct GraphRagClient {
    inner: Client,
    base_url: String,
}

impl GraphRagClient {
    pub fn new(port: u16) -> Self {
        Self {
            inner: Client::new(),
            base_url: format!("http://127.0.0.1:{}", port),
        }
    }

    /// Ingest a single document into the graph index for the given collection.
    pub async fn ingest(
        &self,
        collection_id: &str,
        title: &str,
        content: &str,
    ) -> Result<()> {
        let body = serde_json::json!({
            "title": title,
            "content": content,
            "metadata": { "collection_id": collection_id }
        });
        self.inner
            .post(format!("{}/api/documents", self.base_url))
            .json(&body)
            .send()
            .await
            .context("graphrag ingest request failed")?
            .error_for_status()
            .context("graphrag ingest returned error status")?;
        Ok(())
    }

    /// Query the graph index for a collection.
    pub async fn query(
        &self,
        collection_id: &str,
        q: &str,
        top_k: usize,
    ) -> Result<Vec<GraphResult>> {
        let body = serde_json::json!({
            "query": q,
            "top_k": top_k,
            "filter": { "collection_id": collection_id }
        });
        let resp = self.inner
            .post(format!("{}/api/query", self.base_url))
            .json(&body)
            .send()
            .await
            .context("graphrag query request failed")?
            .error_for_status()
            .context("graphrag query returned error status")?;
        let results: Vec<GraphResult> = resp
            .json()
            .await
            .context("Failed to parse graphrag query response")?;
        Ok(results)
    }

    /// Delete all documents belonging to a collection.
    pub async fn delete_collection(&self, collection_id: &str) -> Result<()> {
        self.inner
            .delete(format!("{}/api/collections/{}", self.base_url, collection_id))
            .send()
            .await
            .context("graphrag delete_collection request failed")?
            .error_for_status()
            .context("graphrag delete_collection returned error status")?;
        Ok(())
    }

    /// Check if the server is up and healthy.
    pub async fn health(&self) -> bool {
        self.inner
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

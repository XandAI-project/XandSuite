pub mod chunker;
pub mod embeddings;
pub mod ingest;
pub mod retriever;

use anyhow::Result;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::db::AppDb;
use crate::graph_rag::GraphRagClient;
use crate::models::{Document, RagCollection, RetrievalMode};
use chunker::{ChunkConfig, chunk_by_paragraphs};
use embeddings::Embedder;
use retriever::{RetrievedChunk, StoredChunk, VectorStore};

pub struct RagService {
    db: Arc<Mutex<AppDb>>,
    vector_store: Mutex<VectorStore>,
}

impl RagService {
    pub fn new(db: Arc<Mutex<AppDb>>, data_dir: &PathBuf) -> Result<Self> {
        let vs_dir = data_dir.join("vectors");
        let vector_store = VectorStore::open(&vs_dir)?;
        Ok(Self {
            db,
            vector_store: Mutex::new(vector_store),
        })
    }

    pub fn create_collection(&self, name: &str, description: Option<&str>) -> Result<RagCollection> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let db = self.db.lock().unwrap();
        db.conn.execute(
            "INSERT INTO rag_collections (id, name, description, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, name, description, now],
        )?;
        Ok(RagCollection {
            id,
            name: name.to_string(),
            description: description.map(String::from),
            document_count: 0,
            created_at: Utc::now(),
            retrieval_mode: RetrievalMode::Hybrid,
            graph_indexed: false,
        })
    }

    pub fn list_collections(&self) -> Result<Vec<RagCollection>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.conn.prepare(
            "SELECT id, name, description, created_at,
                    COALESCE(retrieval_mode, 'hybrid'),
                    COALESCE(graph_indexed, 0)
             FROM rag_collections ORDER BY created_at DESC"
        )?;
        let collection_data: Vec<(String, String, Option<String>, String, String, bool)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);
        drop(db);

        let collections = collection_data
            .into_iter()
            .map(|(id, name, description, created_at, mode_str, graph_indexed)| {
                let vs = self.vector_store.lock().unwrap();
                let count = vs.collection_chunk_count(&id) as u64;
                let retrieval_mode = if mode_str == "graph" {
                    RetrievalMode::Graph
                } else {
                    RetrievalMode::Hybrid
                };
                RagCollection {
                    id,
                    name,
                    description,
                    document_count: count,
                    created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
                    retrieval_mode,
                    graph_indexed,
                }
            })
            .collect();

        Ok(collections)
    }

    /// Update the retrieval mode for a collection.
    pub fn set_retrieval_mode(
        &self,
        collection_id: &str,
        mode: RetrievalMode,
        graph_indexed: bool,
    ) -> Result<()> {
        let mode_str = match mode {
            RetrievalMode::Hybrid => "hybrid",
            RetrievalMode::Graph => "graph",
        };
        let db = self.db.lock().unwrap();
        db.conn.execute(
            "UPDATE rag_collections SET retrieval_mode = ?1, graph_indexed = ?2 WHERE id = ?3",
            rusqlite::params![mode_str, graph_indexed, collection_id],
        )?;
        Ok(())
    }

    pub async fn ingest_file(
        &self,
        collection_id: &str,
        file_path: &Path,
        embedder: &Embedder,
    ) -> Result<Vec<Document>> {
        let raw_records = ingest::ingest_file(file_path).await?;
        let chunk_config = ChunkConfig::default();
        let mut documents = Vec::new();
        let mut all_chunks = Vec::new();

        for (content, metadata) in raw_records {
            let doc_id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            let source = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            {
                let db = self.db.lock().unwrap();
                db.conn.execute(
                    "INSERT INTO rag_documents (id, collection_id, source_file, content, metadata, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        doc_id, collection_id, source,
                        content, metadata.to_string(), now
                    ],
                )?;
            }

            documents.push(Document {
                id: doc_id.clone(),
                collection_id: collection_id.to_string(),
                source_file: source,
                content: content.clone(),
                metadata: metadata.clone(),
                created_at: Utc::now(),
            });

            let text_chunks = chunk_by_paragraphs(&content, &chunk_config);
            let texts: Vec<&str> = text_chunks.iter().map(|s| s.as_str()).collect();
            let embeddings = embedder.embed_batch(&texts).await.unwrap_or_else(|e| {
                log::warn!("Embedding failed during ingest_file: {}. Storing zero vectors.", e);
                text_chunks.iter().map(|_| vec![0.0f32; embedder.dim]).collect()
            });

            for (idx, (chunk_text, embedding)) in text_chunks.iter().zip(embeddings).enumerate() {
                let mut emb = embedding;
                embeddings::normalize(&mut emb);
                all_chunks.push(StoredChunk {
                    id: Uuid::new_v4().to_string(),
                    document_id: doc_id.clone(),
                    collection_id: collection_id.to_string(),
                    content: chunk_text.clone(),
                    chunk_index: idx as u32,
                    metadata: metadata.clone(),
                    embedding: emb,
                });
            }
        }

        let mut vs = self.vector_store.lock().unwrap();
        vs.add_chunks(all_chunks)?;

        Ok(documents)
    }

    pub async fn ingest_text(
        &self,
        collection_id: &str,
        text: &str,
        source: &str,
        embedder: &Embedder,
    ) -> Result<()> {
        let doc_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        {
            let db = self.db.lock().unwrap();
            db.conn.execute(
                "INSERT INTO rag_documents (id, collection_id, source_file, content, metadata, created_at)
                 VALUES (?1, ?2, ?3, ?4, '{}', ?5)",
                rusqlite::params![doc_id, collection_id, source, text, now],
            )?;
        }

        let chunk_config = chunker::ChunkConfig::default();
        let text_chunks = chunk_by_paragraphs(text, &chunk_config);
        let texts: Vec<&str> = text_chunks.iter().map(|s| s.as_str()).collect();
        let embeddings = embedder.embed_batch(&texts).await.unwrap_or_else(|e| {
            log::warn!("Embedding failed during ingest_text: {}. Storing zero vectors.", e);
            text_chunks.iter().map(|_| vec![0.0f32; embedder.dim]).collect()
        });

        let chunks: Vec<retriever::StoredChunk> = text_chunks
            .iter()
            .zip(embeddings)
            .enumerate()
            .map(|(idx, (chunk_text, embedding))| {
                let mut emb = embedding;
                embeddings::normalize(&mut emb);
                retriever::StoredChunk {
                    id: Uuid::new_v4().to_string(),
                    document_id: doc_id.clone(),
                    collection_id: collection_id.to_string(),
                    content: chunk_text.clone(),
                    chunk_index: idx as u32,
                    metadata: serde_json::Value::Object(Default::default()),
                    embedding: emb,
                }
            })
            .collect();

        let mut vs = self.vector_store.lock().unwrap();
        vs.add_chunks(chunks)?;

        Ok(())
    }

    /// Search using hybrid BM25 + cosine similarity.
    pub async fn search(
        &self,
        query: &str,
        collection_id: Option<&str>,
        top_k: usize,
        embedder: &Embedder,
        cosine_weight: f32,
    ) -> Vec<RetrievedChunk> {
        let query_embedding = embedder.embed_one(query).await.unwrap_or_else(|e| {
            log::warn!("Failed to embed query: {}", e);
            vec![]
        });
        let vs = self.vector_store.lock().unwrap();
        vs.search(query, &query_embedding, collection_id, top_k, cosine_weight)
    }

    /// Search, routing to GraphRAG when the collection's retrieval_mode is Graph.
    pub async fn search_with_routing(
        &self,
        query: &str,
        collection_id: Option<&str>,
        top_k: usize,
        embedder: &Embedder,
        cosine_weight: f32,
        graph_client: Option<&GraphRagClient>,
    ) -> Vec<RetrievedChunk> {
        if let Some(col_id) = collection_id {
            if let Some(client) = graph_client {
                // Check if the collection prefers graph mode
                let mode_is_graph = self
                    .list_collections()
                    .ok()
                    .and_then(|cols| cols.into_iter().find(|c| c.id == col_id))
                    .map(|c| c.retrieval_mode == RetrievalMode::Graph && c.graph_indexed)
                    .unwrap_or(false);

                if mode_is_graph && client.health().await {
                    return match client.query(col_id, query, top_k).await {
                        Ok(graph_results) => graph_results
                            .into_iter()
                            .map(|gr| {
                                let chunk = StoredChunk {
                                    id: gr.id.clone(),
                                    document_id: gr.id.clone(),
                                    collection_id: col_id.to_string(),
                                    content: gr.content.clone(),
                                    chunk_index: 0,
                                    metadata: serde_json::json!({ "source": gr.source }),
                                    embedding: vec![],
                                };
                                RetrievedChunk {
                                    chunk,
                                    score: gr.score,
                                    entities: gr.entities,
                                    relationships: gr.relationships,
                                }
                            })
                            .collect(),
                        Err(e) => {
                            log::warn!("GraphRAG query failed, falling back to hybrid: {}", e);
                            self.search(query, Some(col_id), top_k, embedder, cosine_weight).await
                        }
                    };
                }
            }
        }
        self.search(query, collection_id, top_k, embedder, cosine_weight).await
    }

    /// Build a context prompt and also return the source chunks for attribution.
    pub async fn build_rag_context(
        &self,
        query: &str,
        collection_id: Option<&str>,
        embedder: &Embedder,
        cosine_weight: f32,
    ) -> (String, Vec<RetrievedChunk>) {
        let chunks = self.search(query, collection_id, 8, embedder, cosine_weight).await;
        if chunks.is_empty() {
            return (String::new(), vec![]);
        }
        let relevant: Vec<_> = chunks.into_iter().filter(|c| c.score >= 0.1).collect();
        if relevant.is_empty() {
            return (String::new(), vec![]);
        }
        let context = retriever::build_context_prompt(&relevant, 8192);
        (context, relevant)
    }

    /// Like `build_rag_context` but routes to GraphRAG when appropriate.
    pub async fn build_rag_context_routed(
        &self,
        query: &str,
        collection_id: Option<&str>,
        embedder: &Embedder,
        cosine_weight: f32,
        graph_client: Option<&GraphRagClient>,
    ) -> (String, Vec<RetrievedChunk>) {
        let chunks = self
            .search_with_routing(query, collection_id, 8, embedder, cosine_weight, graph_client)
            .await;
        if chunks.is_empty() {
            return (String::new(), vec![]);
        }
        let relevant: Vec<_> = chunks.into_iter().filter(|c| c.score >= 0.1).collect();
        if relevant.is_empty() {
            return (String::new(), vec![]);
        }
        let context = retriever::build_context_prompt(&relevant, 8192);
        (context, relevant)
    }

    pub fn delete_document_chunks(&self, document_id: &str) -> Result<()> {
        let mut vs = self.vector_store.lock().unwrap();
        vs.delete_document(document_id)?;
        Ok(())
    }

    pub fn delete_collection(&self, collection_id: &str) -> Result<()> {
        {
            let db = self.db.lock().unwrap();
            db.conn.execute(
                "DELETE FROM rag_collections WHERE id = ?1",
                rusqlite::params![collection_id],
            )?;
        }
        let mut vs = self.vector_store.lock().unwrap();
        vs.delete_collection(collection_id)?;
        Ok(())
    }
}

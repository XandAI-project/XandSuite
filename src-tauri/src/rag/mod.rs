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
use crate::models::{Document, RagCollection};
use chunker::{ChunkConfig, chunk_by_paragraphs};
use embeddings::embed_text_mock; // kept for StoredChunk.embedding field during ingestion
use retriever::{StoredChunk, VectorStore};

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
        })
    }

    pub fn list_collections(&self) -> Result<Vec<RagCollection>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.conn.prepare(
            "SELECT id, name, description, created_at FROM rag_collections ORDER BY created_at DESC"
        )?;
        let collection_data: Vec<(String, String, Option<String>, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        drop(stmt);
        drop(db);

        let collections = collection_data
            .into_iter()
            .map(|(id, name, description, created_at)| {
                let vs = self.vector_store.lock().unwrap();
                let count = vs.collection_chunk_count(&id) as u64;
                RagCollection {
                    id,
                    name,
                    description,
                    document_count: count,
                    created_at: created_at.parse().unwrap_or_else(|_| Utc::now()),
                }
            })
            .collect();

        Ok(collections)
    }

    pub async fn ingest_file(
        &self,
        collection_id: &str,
        file_path: &Path,
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
            for (idx, chunk_text) in text_chunks.iter().enumerate() {
                let embedding = embed_text_mock(chunk_text);
                all_chunks.push(StoredChunk {
                    id: Uuid::new_v4().to_string(),
                    document_id: doc_id.clone(),
                    collection_id: collection_id.to_string(),
                    content: chunk_text.clone(),
                    chunk_index: idx as u32,
                    metadata: metadata.clone(),
                    embedding,
                });
            }
        }

        let mut vs = self.vector_store.lock().unwrap();
        vs.add_chunks(all_chunks)?;

        Ok(documents)
    }

    pub fn search(
        &self,
        query: &str,
        collection_id: Option<&str>,
        top_k: usize,
    ) -> Vec<retriever::RetrievedChunk> {
        let vs = self.vector_store.lock().unwrap();
        vs.search(query, collection_id, top_k)
    }

    pub fn build_rag_context(&self, query: &str, collection_id: Option<&str>) -> String {
        // Return up to 8 best-matching chunks; allow up to 8 KB of context
        let chunks = self.search(query, collection_id, 8);
        if chunks.is_empty() {
            return String::new();
        }
        // Drop chunks with a normalised score below the threshold to avoid
        // injecting completely unrelated content into the prompt.
        let relevant: Vec<_> = chunks.into_iter().filter(|c| c.score >= 0.15).collect();
        if relevant.is_empty() {
            return String::new();
        }
        retriever::build_context_prompt(&relevant, 8192)
    }

    /// Ingest a raw text string into a collection without going through a file.
    /// `source` is stored in `source_file` for provenance (e.g. conversation ID).
    pub fn ingest_text(
        &self,
        collection_id: &str,
        text: &str,
        source: &str,
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
        let chunks: Vec<retriever::StoredChunk> = text_chunks
            .iter()
            .enumerate()
            .map(|(idx, chunk_text)| {
                let embedding = embed_text_mock(chunk_text);
                retriever::StoredChunk {
                    id: Uuid::new_v4().to_string(),
                    document_id: doc_id.clone(),
                    collection_id: collection_id.to_string(),
                    content: chunk_text.clone(),
                    chunk_index: idx as u32,
                    metadata: serde_json::Value::Object(Default::default()),
                    embedding,
                }
            })
            .collect();

        let mut vs = self.vector_store.lock().unwrap();
        vs.add_chunks(chunks)?;

        Ok(())
    }

    /// Remove all vector-store chunks that belong to a single document.
    /// The DB row should already be deleted by the caller.
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

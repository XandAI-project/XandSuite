use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::embeddings::cosine_similarity;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredChunk {
    pub id: String,
    pub document_id: String,
    pub collection_id: String,
    pub content: String,
    pub chunk_index: u32,
    pub metadata: serde_json::Value,
    /// Real L2-normalised embedding vector (dim matches the configured model).
    pub embedding: Vec<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VectorIndex {
    chunks: Vec<StoredChunk>,
}

pub struct VectorStore {
    index_path: PathBuf,
    chunks: Vec<StoredChunk>,
}

impl VectorStore {
    pub fn open(data_dir: &PathBuf) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .context("Failed to create vector store directory")?;
        let index_path = data_dir.join("vector_index.json");

        let chunks = if index_path.exists() {
            let json = std::fs::read_to_string(&index_path)
                .context("Failed to read vector index")?;
            let idx: VectorIndex = serde_json::from_str(&json)
                .unwrap_or(VectorIndex { chunks: vec![] });
            idx.chunks
        } else {
            vec![]
        };

        Ok(Self { index_path, chunks })
    }

    pub fn add_chunk(&mut self, chunk: StoredChunk) -> Result<()> {
        self.chunks.push(chunk);
        self.persist()
    }

    pub fn add_chunks(&mut self, new_chunks: Vec<StoredChunk>) -> Result<()> {
        self.chunks.extend(new_chunks);
        self.persist()
    }

    /// Hybrid search combining cosine similarity and BM25.
    ///
    /// * `query_embedding` — pre-computed query embedding (may be empty vec if
    ///   the embedder is unavailable; falls back to BM25-only in that case).
    /// * `cosine_weight` — fraction of the final score coming from cosine
    ///   similarity (0.0–1.0).  The remaining fraction comes from BM25.
    pub fn search(
        &self,
        query: &str,
        query_embedding: &[f32],
        collection_id: Option<&str>,
        top_k: usize,
        cosine_weight: f32,
    ) -> Vec<RetrievedChunk> {
        let candidates: Vec<&StoredChunk> = self
            .chunks
            .iter()
            .filter(|c| {
                !c.content.trim().is_empty()
                    && collection_id.map(|id| c.collection_id == id).unwrap_or(true)
            })
            .collect();

        if candidates.is_empty() {
            return vec![];
        }

        let query_terms = tokenize_query(query);
        let use_bm25 = !query_terms.is_empty();
        let use_cosine = !query_embedding.is_empty() && cosine_weight > 0.0;

        // Pre-tokenize all chunks for BM25
        let tokenized: Vec<Vec<String>> = if use_bm25 {
            candidates.iter().map(|c| tokenize(&c.content)).collect()
        } else {
            vec![]
        };

        let bm25_weight = 1.0 - cosine_weight.clamp(0.0, 1.0);

        // BM25 parameters
        let avg_doc_len = if use_bm25 && !tokenized.is_empty() {
            tokenized.iter().map(|t| t.len()).sum::<usize>() as f32 / tokenized.len() as f32
        } else {
            1.0
        };

        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        if use_bm25 {
            for tokens in &tokenized {
                let unique: std::collections::HashSet<&String> = tokens.iter().collect();
                for term in unique {
                    *doc_freq.entry(term.clone()).or_insert(0) += 1;
                }
            }
        }
        let total_docs = candidates.len();

        // Score every candidate
        let mut scored: Vec<(f32, &'_ StoredChunk)> = candidates
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let cosine_score = if use_cosine && !chunk.embedding.is_empty() {
                    cosine_similarity(query_embedding, &chunk.embedding).max(0.0)
                } else {
                    0.0
                };

                let bm25_score = if use_bm25 {
                    bm25_score(
                        &query_terms,
                        &tokenized[i],
                        tokenized[i].len(),
                        avg_doc_len,
                        total_docs,
                        &doc_freq,
                    )
                } else {
                    0.0
                };

                (cosine_score, bm25_score, *chunk)
            })
            .collect::<Vec<_>>()
            // Normalise BM25 scores to [0,1] so they are on the same scale as cosine
            .pipe_bm25_normalise(use_bm25)
            .into_iter()
            .map(|(cos, bm25, chunk)| {
                let score = if use_cosine && use_bm25 {
                    cosine_weight * cos + bm25_weight * bm25
                } else if use_cosine {
                    cos
                } else {
                    bm25
                };
                (score, chunk)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        scored
            .into_iter()
            .filter(|(score, _)| *score > 0.0)
            .take(top_k)
            .map(|(score, chunk)| RetrievedChunk {
                chunk: chunk.clone(),
                score,
                entities: vec![],
                relationships: vec![],
            })
            .collect()
    }

    pub fn delete_collection(&mut self, collection_id: &str) -> Result<usize> {
        let before = self.chunks.len();
        self.chunks.retain(|c| c.collection_id != collection_id);
        let removed = before - self.chunks.len();
        self.persist()?;
        Ok(removed)
    }

    pub fn delete_document(&mut self, document_id: &str) -> Result<usize> {
        let before = self.chunks.len();
        self.chunks.retain(|c| c.document_id != document_id);
        let removed = before - self.chunks.len();
        self.persist()?;
        Ok(removed)
    }

    pub fn collection_chunk_count(&self, collection_id: &str) -> usize {
        self.chunks
            .iter()
            .filter(|c| c.collection_id == collection_id)
            .count()
    }

    fn persist(&self) -> Result<()> {
        let idx = VectorIndex {
            chunks: self.chunks.clone(),
        };
        let json = serde_json::to_string(&idx)?;
        std::fs::write(&self.index_path, json)
            .context("Failed to persist vector index")?;
        Ok(())
    }
}

// ── Normalisation helper (free function on Vec to keep score_loop readable) ──

trait NormaliseBm25 {
    fn pipe_bm25_normalise(self, active: bool) -> Self;
}

impl<'a> NormaliseBm25 for Vec<(f32, f32, &'a StoredChunk)> {
    fn pipe_bm25_normalise(mut self, active: bool) -> Self {
        if !active {
            return self;
        }
        let max_bm25 = self.iter().map(|(_, b, _)| *b).fold(0.0_f32, f32::max).max(1e-6);
        for (_, bm25, _) in self.iter_mut() {
            *bm25 /= max_bm25;
        }
        self
    }
}

// ── BM25 helpers ──────────────────────────────────────────────────────────────

const STOP_WORDS: &[&str] = &[
    "de", "da", "do", "das", "dos", "em", "na", "no", "nas", "nos",
    "ao", "aos", "às", "um", "uma", "uns", "umas",
    "que", "para", "com", "por", "mas", "ou", "se", "já",
    "seu", "sua", "seus", "suas", "meu", "minha", "meus", "minhas",
    "este", "esta", "estes", "estas", "esse", "essa", "esses", "essas",
    "aquele", "aquela", "aqueles", "aquelas",
    "ele", "ela", "eles", "elas", "eu", "tu", "nos", "vos",
    "me", "te", "lhe", "lhes",
    "ser", "ter", "foi", "era", "são", "tem",
    "fale", "falar", "diga", "dizer", "conte", "contar",
    "seja", "breve", "então", "sobre", "mais", "bem", "muito",
    "como", "quando", "onde", "quem", "qual", "quais",
    "também", "ainda", "até", "depois", "antes", "sempre", "nunca",
    "há", "vai", "vem", "pode", "deve", "quer",
    "the", "a", "an", "and", "or", "is", "are", "was", "were",
    "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should",
    "may", "might", "must", "can", "to", "of", "in", "for",
    "on", "with", "at", "by", "from", "up", "about", "into",
    "tell", "me", "who", "what", "when", "where", "how",
    "brief", "briefly", "please",
];

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .map(|s| s.to_string())
        .collect()
}

fn tokenize_query(text: &str) -> Vec<String> {
    tokenize(text)
        .into_iter()
        .filter(|t| !STOP_WORDS.contains(&t.as_str()))
        .collect()
}

fn bm25_score(
    query_terms: &[String],
    doc_tokens: &[String],
    doc_len: usize,
    avg_doc_len: f32,
    total_docs: usize,
    doc_freq: &HashMap<String, usize>,
) -> f32 {
    const K1: f32 = 1.5;
    const B: f32 = 0.75;

    let mut score = 0.0_f32;
    for term in query_terms {
        let tf = doc_tokens.iter().filter(|t| *t == term).count() as f32;
        if tf == 0.0 { continue; }
        let df = *doc_freq.get(term).unwrap_or(&1) as f32;
        let n = total_docs as f32;
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
        let norm = 1.0 - B + B * (doc_len as f32 / avg_doc_len.max(1.0));
        let tf_norm = (tf * (K1 + 1.0)) / (tf + K1 * norm);
        score += idf * tf_norm;
    }
    score
}

// ── Result types ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RetrievedChunk {
    pub chunk: StoredChunk,
    /// Combined relevance score in [0, 1].
    pub score: f32,
    /// Entities extracted by the graph pipeline (empty for hybrid mode).
    pub entities: Vec<String>,
    /// Relationships extracted by the graph pipeline (empty for hybrid mode).
    pub relationships: Vec<String>,
}

// ── Context builder ───────────────────────────────────────────────────────────

pub fn build_context_prompt(chunks: &[RetrievedChunk], max_chars: usize) -> String {
    let mut context = String::from("Relevant context from the knowledge base:\n\n");
    let mut total = context.len();

    for (i, retrieved) in chunks.iter().enumerate() {
        let source = retrieved
            .chunk
            .metadata
            .get("source")
            .and_then(|v| v.as_str())
            .or_else(|| retrieved.chunk.metadata.get("source_file").and_then(|v| v.as_str()))
            .unwrap_or("document");

        let entity_line = if !retrieved.entities.is_empty() {
            format!("\n  entities: {}", retrieved.entities.join(", "))
        } else {
            String::new()
        };

        let entry = format!(
            "[{}] (source: {}{})\n{}\n\n",
            i + 1,
            source,
            entity_line,
            retrieved.chunk.content
        );

        if total + entry.len() > max_chars {
            break;
        }
        context.push_str(&entry);
        total += entry.len();
    }

    context
}

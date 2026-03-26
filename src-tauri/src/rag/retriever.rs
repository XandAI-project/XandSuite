use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredChunk {
    pub id: String,
    pub document_id: String,
    pub collection_id: String,
    pub content: String,
    pub chunk_index: u32,
    pub metadata: serde_json::Value,
    // kept for schema compatibility; no longer used for retrieval
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

    /// BM25-based full-text search.
    /// Works correctly with natural language queries (any language) without
    /// requiring a separate embedding model.
    pub fn search(
        &self,
        query: &str,
        collection_id: Option<&str>,
        top_k: usize,
    ) -> Vec<RetrievedChunk> {
        // Candidate chunks for this collection
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
        if query_terms.is_empty() {
            return vec![];
        }

        // Pre-tokenize all candidate chunks
        let tokenized: Vec<Vec<String>> =
            candidates.iter().map(|c| tokenize(&c.content)).collect();

        // Average document length
        let avg_doc_len =
            tokenized.iter().map(|t| t.len()).sum::<usize>() as f32 / tokenized.len() as f32;

        // Document frequency: how many chunks contain each term
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for tokens in &tokenized {
            let unique: std::collections::HashSet<&String> = tokens.iter().collect();
            for term in unique {
                *doc_freq.entry(term.clone()).or_insert(0) += 1;
            }
        }

        let total_docs = candidates.len();

        // Score every candidate with BM25
        let mut scored: Vec<(f32, &StoredChunk)> = candidates
            .iter()
            .zip(tokenized.iter())
            .map(|(chunk, tokens)| {
                let score = bm25_score(
                    &query_terms,
                    tokens,
                    tokens.len(),
                    avg_doc_len,
                    total_docs,
                    &doc_freq,
                );
                (score, *chunk)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Normalise scores to 0-1 using the max score so the UI percentage
        // is meaningful (100 % = best matching chunk in this result set).
        let max_score = scored.first().map(|(s, _)| *s).unwrap_or(1.0).max(1e-6);

        scored
            .into_iter()
            // Only return chunks that actually matched at least one query term
            .filter(|(score, _)| *score > 0.0)
            .take(top_k)
            .map(|(score, chunk)| RetrievedChunk {
                chunk: chunk.clone(),
                score: score / max_score,
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

// ── BM25 helpers ──────────────────────────────────────────────────────────────

/// Common Portuguese and English stop words stripped from *queries* only.
/// Documents are still indexed with all tokens so nothing is lost at ingest time.
const STOP_WORDS: &[&str] = &[
    // Portuguese — function words, common verbs, prepositions, pronouns
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
    // English — common function words
    "the", "a", "an", "and", "or", "is", "are", "was", "were",
    "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should",
    "may", "might", "must", "can", "to", "of", "in", "for",
    "on", "with", "at", "by", "from", "up", "about", "into",
    "tell", "me", "about", "who", "what", "when", "where", "how",
    "brief", "briefly", "please",
];

/// Tokenise text: lowercase, split on non-alphanumeric, filter single chars.
/// Language-agnostic — works with Portuguese, English, etc.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() > 1)
        .map(|s| s.to_string())
        .collect()
}

/// Like `tokenize` but also strips stop words.
/// Used for queries so that conversational phrases don't dilute BM25 scores
/// for the specific entity or concept the user is asking about.
fn tokenize_query(text: &str) -> Vec<String> {
    tokenize(text)
        .into_iter()
        .filter(|t| !STOP_WORDS.contains(&t.as_str()))
        .collect()
}

/// BM25 (Okapi BM25) scoring for a single document against a query.
///
/// Parameters k1=1.5, b=0.75 are standard defaults.
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
        if tf == 0.0 {
            continue;
        }
        // Document frequency — default to 1 to avoid div-by-zero
        let df = *doc_freq.get(term).unwrap_or(&1) as f32;
        let n = total_docs as f32;

        // IDF with smoothing (avoids negative IDF for very common terms)
        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();

        // Length-normalised TF
        let norm = 1.0 - B + B * (doc_len as f32 / avg_doc_len.max(1.0));
        let tf_norm = (tf * (K1 + 1.0)) / (tf + K1 * norm);

        score += idf * tf_norm;
    }
    score
}

// ── Result type ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct RetrievedChunk {
    pub chunk: StoredChunk,
    /// Normalised relevance score in [0, 1]  (1.0 = best match in result set)
    pub score: f32,
}

// ── Context builder ───────────────────────────────────────────────────────────

/// Assemble retrieved chunks into a context block for the LLM.
/// Each entry includes the source file so the model can cite it.
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

        let entry = format!(
            "[{}] (source: {})\n{}\n\n",
            i + 1,
            source,
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

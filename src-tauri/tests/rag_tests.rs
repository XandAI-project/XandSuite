use xandsuite_lib::rag::chunker::{ChunkConfig, chunk_fixed_size, chunk_by_paragraphs};
use xandsuite_lib::rag::embeddings::{embed_text_mock, cosine_similarity, EMBEDDING_DIM};

#[test]
fn test_chunk_fixed_size_produces_chunks() {
    let text = "Hello world. ".repeat(100);
    let config = ChunkConfig { chunk_size: 100, overlap: 20 };
    let chunks = chunk_fixed_size(&text, &config);
    assert!(!chunks.is_empty(), "Should produce chunks for long text");
}

#[test]
fn test_chunk_fixed_size_empty() {
    let config = ChunkConfig::default();
    let chunks = chunk_fixed_size("", &config);
    assert!(chunks.is_empty());
}

#[test]
fn test_chunk_paragraphs() {
    let text = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph.";
    let config = ChunkConfig::default();
    let chunks = chunk_by_paragraphs(text, &config);
    assert!(!chunks.is_empty());
    assert!(chunks.iter().all(|c| !c.is_empty()));
}

#[test]
fn test_embedding_dimension() {
    let emb = embed_text_mock("test embedding");
    assert_eq!(emb.len(), EMBEDDING_DIM);
}

#[test]
fn test_embedding_is_normalized() {
    let emb = embed_text_mock("normalization test");
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    // Either normalized (norm ~= 1) or zero vector
    assert!(norm < 0.01 || (norm - 1.0).abs() < 0.01, "norm = {}", norm);
}

#[test]
fn test_cosine_similarity_same_vector() {
    let emb = embed_text_mock("identical text");
    let sim = cosine_similarity(&emb, &emb);
    // Self-similarity should be ~1.0
    assert!(sim > 0.99, "sim = {}", sim);
}

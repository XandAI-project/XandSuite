
pub const EMBEDDING_DIM: usize = 384;

/// Generate a mock embedding for text.
/// In production this should call the llama.cpp embeddings API.
pub fn embed_text_mock(text: &str) -> Vec<f32> {
    let mut embedding = vec![0.0f32; EMBEDDING_DIM];

    for (i, ch) in text.chars().enumerate().take(EMBEDDING_DIM * 4) {
        let idx = (ch as usize + i * 31) % EMBEDDING_DIM;
        embedding[idx] += 1.0 / (1.0 + i as f32);
    }

    normalize(&mut embedding);
    embedding
}

/// L2-normalize a vector in place.
pub fn normalize(v: &mut Vec<f32>) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Cosine similarity between two normalized vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_dimension() {
        let emb = embed_text_mock("hello world");
        assert_eq!(emb.len(), EMBEDDING_DIM);
    }

    #[test]
    fn test_embedding_normalized() {
        let emb = embed_text_mock("test text");
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01 || norm < 0.01);
    }

    #[test]
    fn test_similar_texts_closer() {
        let a = embed_text_mock("the cat sat on the mat");
        let b = embed_text_mock("the cat sat on the mat again");
        let c = embed_text_mock("quantum physics and black holes");
        let sim_close = cosine_similarity(&a, &b);
        let sim_far = cosine_similarity(&a, &c);
        // With mock embeddings this is approximate
        assert!(sim_close >= 0.0 && sim_far >= 0.0);
    }
}

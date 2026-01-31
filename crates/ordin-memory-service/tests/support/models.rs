//! The hand-written deterministic embedder and reranker of the everyday tests (the user's decision
//! of 2026-01-21): `cargo test` stays fast and offline, and the real models have their own job.
//!
//! The embedder hashes each word into a bucket of a 384-dimensional unit vector, so texts sharing
//! words are close. The reranker scores the share of the query's words a document holds.

use async_trait::async_trait;
use memory::types::ports::{EmbeddingClient, ProviderError, RerankClient};

const DIM: usize = 384;
const STOP: [&str; 24] = [
    "a", "an", "and", "are", "at", "do", "does", "for", "how", "in", "is", "of", "on", "or", "the",
    "to", "what", "when", "which", "who", "why", "with", "s", "t",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace("'s", " ")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(w))
        .map(str::to_owned)
        .collect()
}

fn bucket(word: &str) -> usize {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in word.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash % DIM as u64) as usize
}

pub struct WordEmbedder;

#[async_trait]
impl EmbeddingClient for WordEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Ok(texts
            .iter()
            .map(|text| {
                let mut vector = vec![0.0f32; DIM];
                for word in words(text) {
                    vector[bucket(&word)] += 1.0;
                }
                let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
                if norm > 0.0 {
                    vector.iter_mut().for_each(|v| *v /= norm);
                } else {
                    vector[0] = 1.0;
                }
                vector
            })
            .collect())
    }
}

pub struct WordReranker;

#[async_trait]
impl RerankClient for WordReranker {
    async fn rerank(&self, query: &str, docs: &[String]) -> Result<Vec<f64>, ProviderError> {
        let wanted = words(query);
        Ok(docs
            .iter()
            .map(|doc| {
                let held = words(doc);
                if wanted.is_empty() {
                    return 0.0;
                }
                let found = wanted.iter().filter(|w| held.contains(w)).count();
                found as f64 / wanted.len() as f64
            })
            .collect())
    }
}

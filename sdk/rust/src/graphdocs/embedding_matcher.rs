//! Embedding-based status matching for unknown status variants
//!
//! Uses cosine similarity to find the closest known status for unknown variants.

use crate::graphdocs::normalizer::ExtendedStatus;
use anyhow::Result;

/// Known status phrases with pre-computed embeddings
pub struct StatusEmbeddings {
    known_statuses: Vec<(String, Vec<f32>, ExtendedStatus)>,
}

impl Default for StatusEmbeddings {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusEmbeddings {
    /// Initialize with known status phrases
    pub fn new() -> Self {
        Self {
            known_statuses: Vec::new(),
        }
    }

    /// Pre-compute embeddings for known status phrases
    /// Uses TEA's memory.embed action via subprocess
    pub async fn initialize(&mut self) -> Result<()> {
        let known_phrases = vec![
            ("Done", ExtendedStatus::Done),
            ("Complete", ExtendedStatus::Done),
            ("Finished", ExtendedStatus::Done),
            ("Completed successfully", ExtendedStatus::Done),
            ("Development complete", ExtendedStatus::Review),
            ("Ready for review", ExtendedStatus::Review),
            ("In progress", ExtendedStatus::InProgress),
            ("Work in progress", ExtendedStatus::InProgress),
            ("Currently working", ExtendedStatus::InProgress),
            ("Draft", ExtendedStatus::Draft),
            ("Not started", ExtendedStatus::Draft),
            ("Planned", ExtendedStatus::Draft),
            ("Approved", ExtendedStatus::Approved),
            ("Ready for development", ExtendedStatus::Approved),
        ];

        for (phrase, status) in known_phrases {
            let embedding = self.embed_text(phrase).await?;
            self.known_statuses
                .push((phrase.to_string(), embedding, status));
        }

        Ok(())
    }

    /// Add a known status with its embedding
    pub fn add_known_status(&mut self, phrase: &str, embedding: Vec<f32>, status: ExtendedStatus) {
        self.known_statuses
            .push((phrase.to_string(), embedding, status));
    }

    /// Embed text using TEA CLI for embedding
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        // Call TEA CLI for embedding
        let output = tokio::process::Command::new("tea")
            .args(["embed", "--model", "model2vec", "--text", text])
            .output()
            .await?;

        if !output.status.success() {
            // Return empty embedding if TEA is not available
            return Ok(vec![]);
        }

        let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let embedding: Vec<f32> = result["embedding"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }

    /// Match unknown status text to closest known status
    pub async fn match_status(&self, text: &str) -> Result<(ExtendedStatus, f32)> {
        let embedding = self.embed_text(text).await?;

        if embedding.is_empty() {
            // Fallback: use simple string matching if embedding fails
            return Ok((self.fallback_match(text), 0.0));
        }

        let mut best_match = ExtendedStatus::Draft;
        let mut best_score = 0.0f32;

        for (_, known_emb, status) in &self.known_statuses {
            if known_emb.is_empty() {
                continue;
            }
            let score = cosine_similarity(&embedding, known_emb);
            if score > best_score {
                best_score = score;
                best_match = status.clone();
            }
        }

        Ok((best_match, best_score))
    }

    /// Match status synchronously using pre-computed embeddings
    pub fn match_status_sync(&self, embedding: &[f32]) -> (ExtendedStatus, f32) {
        if embedding.is_empty() {
            return (ExtendedStatus::Draft, 0.0);
        }

        let mut best_match = ExtendedStatus::Draft;
        let mut best_score = 0.0f32;

        for (_, known_emb, status) in &self.known_statuses {
            if known_emb.is_empty() {
                continue;
            }
            let score = cosine_similarity(embedding, known_emb);
            if score > best_score {
                best_score = score;
                best_match = status.clone();
            }
        }

        (best_match, best_score)
    }

    /// Simple string-based fallback matching
    fn fallback_match(&self, text: &str) -> ExtendedStatus {
        let lower = text.to_lowercase();

        if lower.contains("done") || lower.contains("complete") || lower.contains("finish") {
            return ExtendedStatus::Done;
        }
        if lower.contains("progress") || lower.contains("wip") || lower.contains("working") {
            return ExtendedStatus::InProgress;
        }
        if lower.contains("review") || lower.contains("dev complete") {
            return ExtendedStatus::Review;
        }
        if lower.contains("approved") || lower.contains("ready") {
            return ExtendedStatus::Approved;
        }

        ExtendedStatus::Draft
    }
}

/// Compute cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_45_degrees() {
        let a = vec![1.0, 0.0, 0.0];
        let d = vec![0.707, 0.707, 0.0];
        assert!((cosine_similarity(&a, &d) - 0.707).abs() < 0.01);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_status_embeddings_new() {
        let embeddings = StatusEmbeddings::new();
        assert!(embeddings.known_statuses.is_empty());
    }

    #[test]
    fn test_status_embeddings_default() {
        let embeddings = StatusEmbeddings::default();
        assert!(embeddings.known_statuses.is_empty());
    }

    #[test]
    fn test_add_known_status() {
        let mut embeddings = StatusEmbeddings::new();
        embeddings.add_known_status("Test", vec![1.0, 0.0], ExtendedStatus::Done);
        assert_eq!(embeddings.known_statuses.len(), 1);
    }

    #[test]
    fn test_match_status_sync_empty_embedding() {
        let embeddings = StatusEmbeddings::new();
        let (status, score) = embeddings.match_status_sync(&[]);
        assert_eq!(status, ExtendedStatus::Draft);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_match_status_sync_with_known() {
        let mut embeddings = StatusEmbeddings::new();
        embeddings.add_known_status("Done", vec![1.0, 0.0, 0.0], ExtendedStatus::Done);
        embeddings.add_known_status("Progress", vec![0.0, 1.0, 0.0], ExtendedStatus::InProgress);

        // Test matching against "Done" embedding
        let (status, score) = embeddings.match_status_sync(&[1.0, 0.0, 0.0]);
        assert_eq!(status, ExtendedStatus::Done);
        assert!((score - 1.0).abs() < 0.001);

        // Test matching against "Progress" embedding
        let (status, score) = embeddings.match_status_sync(&[0.0, 1.0, 0.0]);
        assert_eq!(status, ExtendedStatus::InProgress);
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_fallback_match() {
        let embeddings = StatusEmbeddings::new();

        assert_eq!(embeddings.fallback_match("Done"), ExtendedStatus::Done);
        assert_eq!(embeddings.fallback_match("completed"), ExtendedStatus::Done);
        assert_eq!(
            embeddings.fallback_match("In Progress"),
            ExtendedStatus::InProgress
        );
        assert_eq!(embeddings.fallback_match("WIP"), ExtendedStatus::InProgress);
        assert_eq!(
            embeddings.fallback_match("Ready for Review"),
            ExtendedStatus::Review
        );
        assert_eq!(
            embeddings.fallback_match("Approved"),
            ExtendedStatus::Approved
        );
        assert_eq!(embeddings.fallback_match("Unknown"), ExtendedStatus::Draft);
    }
}

//! Vector similarity and hybrid search utilities.
//!
//! Pure-Rust implementations of:
//! - Cosine similarity
//! - Reciprocal Rank Fusion (RRF) for merging ranked result lists

use rustedclaw_core::memory::MemoryEntry;

/// Compute cosine similarity between two vectors.
///
/// Returns a value in [-1, 1] where 1 = identical, 0 = orthogonal, -1 = opposite.
/// Returns 0.0 if either vector is zero-length or empty.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;

    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-10 {
        return 0.0;
    }

    (dot / denom) as f32
}

/// Rank entries by cosine similarity to a query embedding.
///
/// Returns entries sorted by descending similarity, with `score` set to the
/// cosine similarity value. Only entries that have embeddings and meet the
/// minimum score threshold are included.
pub fn vector_search(
    entries: &[MemoryEntry],
    query_embedding: &[f32],
    limit: usize,
    min_score: f32,
) -> Vec<MemoryEntry> {
    let mut scored: Vec<(f32, MemoryEntry)> = entries
        .iter()
        .filter_map(|entry| {
            let emb = entry.embedding.as_ref()?;
            let sim = cosine_similarity(emb, query_embedding);
            if sim >= min_score {
                let mut e = entry.clone();
                e.score = sim;
                Some((sim, e))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored.into_iter().map(|(_, e)| e).collect()
}

/// Reciprocal Rank Fusion (RRF) — merge two ranked result lists.
///
/// Each entry's final score = sum of 1/(k + rank) across both lists.
/// The constant k controls how much weight is given to lower-ranked items.
/// Standard value is k=60.
///
/// Returns merged results sorted by RRF score, deduplicated by entry ID.
pub fn reciprocal_rank_fusion(
    keyword_results: &[MemoryEntry],
    vector_results: &[MemoryEntry],
    k: u32,
    limit: usize,
) -> Vec<MemoryEntry> {
    use std::collections::HashMap;

    let k = k as f32;

    // Map: id → (rrf_score, best_entry)
    let mut scores: HashMap<String, (f32, MemoryEntry)> = HashMap::new();

    // Score keyword results
    for (rank, entry) in keyword_results.iter().enumerate() {
        let rrf_score = 1.0 / (k + rank as f32 + 1.0);
        scores
            .entry(entry.id.clone())
            .and_modify(|(score, _)| *score += rrf_score)
            .or_insert_with(|| (rrf_score, entry.clone()));
    }

    // Score vector results
    for (rank, entry) in vector_results.iter().enumerate() {
        let rrf_score = 1.0 / (k + rank as f32 + 1.0);
        scores
            .entry(entry.id.clone())
            .and_modify(|(score, _)| *score += rrf_score)
            .or_insert_with(|| (rrf_score, entry.clone()));
    }

    // Collect, sort by RRF score descending
    let mut results: Vec<MemoryEntry> = scores
        .into_values()
        .map(|(score, mut entry)| {
            entry.score = score;
            entry
        })
        .collect();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn entry(id: &str, embedding: Option<Vec<f32>>) -> MemoryEntry {
        MemoryEntry {
            id: id.into(),
            content: format!("Content for {id}"),
            tags: vec![],
            source: None,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            score: 0.0,
            embedding,
        }
    }

    #[test]
    fn cosine_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn cosine_opposite_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn cosine_empty_vectors() {
        let sim = cosine_similarity(&[], &[]);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn cosine_mismatched_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn cosine_zero_vector() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn cosine_known_value() {
        // [1,1] · [1,0] = 1, |[1,1]| = sqrt(2), |[1,0]| = 1
        // similarity = 1 / sqrt(2) ≈ 0.7071
        let a = vec![1.0, 1.0];
        let b = vec![1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.7071).abs() < 0.001);
    }

    #[test]
    fn vector_search_ranks_by_similarity() {
        let query = vec![1.0, 0.0, 0.0];
        let entries = vec![
            entry("a", Some(vec![0.0, 1.0, 0.0])), // orthogonal = 0
            entry("b", Some(vec![1.0, 0.0, 0.0])), // identical = 1
            entry("c", Some(vec![0.5, 0.5, 0.0])), // partial = ~0.707
        ];

        let results = vector_search(&entries, &query, 10, 0.0);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, "b"); // highest similarity
        assert_eq!(results[1].id, "c");
        assert_eq!(results[2].id, "a");
    }

    #[test]
    fn vector_search_respects_min_score() {
        let query = vec![1.0, 0.0];
        let entries = vec![
            entry("a", Some(vec![1.0, 0.0])),  // sim = 1.0
            entry("b", Some(vec![0.0, 1.0])),  // sim = 0.0
        ];

        let results = vector_search(&entries, &query, 10, 0.5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
    }

    #[test]
    fn vector_search_skips_no_embedding() {
        let query = vec![1.0, 0.0];
        let entries = vec![
            entry("a", Some(vec![1.0, 0.0])),
            entry("b", None), // no embedding
        ];

        let results = vector_search(&entries, &query, 10, 0.0);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
    }

    #[test]
    fn vector_search_respects_limit() {
        let query = vec![1.0, 0.0];
        let entries: Vec<_> = (0..10)
            .map(|i| entry(&format!("e{i}"), Some(vec![1.0, i as f32 * 0.1])))
            .collect();

        let results = vector_search(&entries, &query, 3, 0.0);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn rrf_merges_two_lists() {
        let keyword = vec![
            entry("a", None), // rank 0
            entry("b", None), // rank 1
            entry("c", None), // rank 2
        ];
        let vector = vec![
            entry("b", None), // rank 0
            entry("d", None), // rank 1
            entry("a", None), // rank 2
        ];

        let results = reciprocal_rank_fusion(&keyword, &vector, 60, 10);

        // "b" appears at rank 1 in keyword + rank 0 in vector = highest combined
        // "a" appears at rank 0 in keyword + rank 2 in vector = second
        assert!(!results.is_empty());
        // b should be first (appears early in both lists)
        assert_eq!(results[0].id, "b");
        assert_eq!(results[1].id, "a");
    }

    #[test]
    fn rrf_deduplicates() {
        let list = vec![entry("x", None), entry("y", None)];
        let results = reciprocal_rank_fusion(&list, &list, 60, 10);
        assert_eq!(results.len(), 2); // no duplicates
    }

    #[test]
    fn rrf_respects_limit() {
        let keyword: Vec<_> = (0..20)
            .map(|i| entry(&format!("k{i}"), None))
            .collect();
        let vector: Vec<_> = (0..20)
            .map(|i| entry(&format!("v{i}"), None))
            .collect();

        let results = reciprocal_rank_fusion(&keyword, &vector, 60, 5);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn rrf_empty_lists() {
        let results = reciprocal_rank_fusion(&[], &[], 60, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn rrf_one_empty_list() {
        let keyword = vec![entry("a", None), entry("b", None)];
        let results = reciprocal_rank_fusion(&keyword, &[], 60, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "a"); // rank 0 in keyword = highest
    }
}

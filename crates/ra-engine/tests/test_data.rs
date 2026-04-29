//! Test data generators for hybrid search integration tests.
//!
//! Provides utilities for generating synthetic documents, embeddings,
//! queries, and expected results for testing hybrid search functionality.
#![expect(
    clippy::float_cmp,
    clippy::unwrap_used,
    reason = "test code"
)]
#![allow(dead_code)]

use rand::rngs::StdRng;
use rand::Rng;
use rand::SeedableRng;

/// A synthetic document with text content and vector embedding.
#[derive(Debug, Clone)]
pub struct TestDocument {
    pub id: usize,
    pub title: String,
    pub content: String,
    pub embedding: Vec<f64>,
    pub category: String,
}

/// A test query with both text and vector components.
#[derive(Debug, Clone)]
pub struct TestQuery {
    pub text: String,
    pub embedding: Vec<f64>,
    pub expected_fts_selectivity: f64,
    pub expected_vector_selectivity: f64,
}

/// Expected result for validation.
#[derive(Debug, Clone)]
pub struct ExpectedResult {
    pub doc_id: usize,
    pub expected_rank_range: (usize, usize),
    pub min_score: f64,
}

/// Generate synthetic documents with embeddings.
///
/// Creates documents with:
/// - Sequential IDs
/// - Random titles and content from a vocabulary
/// - Random embeddings in the specified dimension
/// - Category labels for filtering
#[must_use]
pub fn generate_documents(count: usize, embedding_dim: usize, seed: u64) -> Vec<TestDocument> {
    let mut rng = StdRng::seed_from_u64(seed);
    let categories = [
        "technology",
        "science",
        "sports",
        "politics",
        "entertainment",
    ];
    let words = vec![
        "machine",
        "learning",
        "algorithm",
        "data",
        "neural",
        "network",
        "optimization",
        "query",
        "database",
        "search",
        "vector",
        "embedding",
        "similarity",
        "distance",
        "ranking",
        "relevance",
        "score",
        "fusion",
    ];

    (0..count)
        .map(|id| {
            let title_words = (0..5)
                .map(|_| words[rng.gen_range(0..words.len())])
                .collect::<Vec<_>>()
                .join(" ");

            let content_words = (0..50)
                .map(|_| words[rng.gen_range(0..words.len())])
                .collect::<Vec<_>>()
                .join(" ");

            let embedding = (0..embedding_dim)
                .map(|_| rng.gen_range(-1.0..1.0))
                .collect();

            let category = categories[id % categories.len()].to_string();

            TestDocument {
                id,
                title: title_words,
                content: content_words,
                embedding,
                category,
            }
        })
        .collect()
}

/// Generate a query with high FTS selectivity (rare terms).
#[must_use]
pub fn generate_high_fts_selectivity_query(embedding_dim: usize) -> TestQuery {
    TestQuery {
        text: "machine learning optimization neural network".to_string(),
        embedding: (0..embedding_dim).map(|i| (i as f64) / 100.0).collect(),
        expected_fts_selectivity: 0.005,  // 0.5% of documents match
        expected_vector_selectivity: 0.1, // 10% of documents match
    }
}

/// Generate a query with high vector selectivity (precise embedding).
#[must_use]
pub fn generate_high_vector_selectivity_query(embedding_dim: usize) -> TestQuery {
    TestQuery {
        text: "data query search".to_string(),
        embedding: vec![1.0; embedding_dim],
        expected_fts_selectivity: 0.15,     // 15% of documents match
        expected_vector_selectivity: 0.003, // 0.3% of documents match
    }
}

/// Generate a query with similar selectivities (balanced).
#[must_use]
pub fn generate_balanced_query(embedding_dim: usize) -> TestQuery {
    TestQuery {
        text: "search algorithm database".to_string(),
        embedding: (0..embedding_dim)
            .map(|i| ((i % 10) as f64) / 10.0)
            .collect(),
        expected_fts_selectivity: 0.05,    // 5% of documents match
        expected_vector_selectivity: 0.06, // 6% of documents match
    }
}

/// Generate queries with varying selectivity for testing strategy selection.
#[must_use]
pub fn generate_varied_queries(embedding_dim: usize) -> Vec<TestQuery> {
    vec![
        generate_high_fts_selectivity_query(embedding_dim),
        generate_high_vector_selectivity_query(embedding_dim),
        generate_balanced_query(embedding_dim),
        // Very broad query
        TestQuery {
            text: "data".to_string(),
            embedding: vec![0.0; embedding_dim],
            expected_fts_selectivity: 0.8,
            expected_vector_selectivity: 0.9,
        },
        // Very narrow query
        TestQuery {
            text: "machine learning neural network optimization algorithm".to_string(),
            embedding: vec![0.99; embedding_dim],
            expected_fts_selectivity: 0.0001,
            expected_vector_selectivity: 0.0001,
        },
    ]
}

/// Calculate L2 distance between two vectors.
#[must_use]
pub fn l2_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// Calculate cosine similarity between two vectors.
#[must_use]
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot_product: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f64 = a.iter().map(|x| x.powi(2)).sum::<f64>().sqrt();
    let magnitude_b: f64 = b.iter().map(|x| x.powi(2)).sum::<f64>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        0.0
    } else {
        dot_product / (magnitude_a * magnitude_b)
    }
}

/// Calculate inner product between two vectors.
#[must_use]
pub fn inner_product(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Calculate simple BM25-like score for testing.
///
/// Simplified BM25 that counts term matches without IDF.
#[must_use]
pub fn simple_bm25_score(doc_text: &str, query_text: &str) -> f64 {
    let doc_words: Vec<&str> = doc_text.split_whitespace().collect();
    let query_words: Vec<&str> = query_text.split_whitespace().collect();

    if doc_words.is_empty() {
        return 0.0;
    }

    let mut score = 0.0;
    for query_word in &query_words {
        let term_freq = doc_words.iter().filter(|w| w == &query_word).count() as f64;
        if term_freq > 0.0 {
            // Simplified BM25 formula (k1=1.2, b=0.75)
            let k1 = 1.2;
            let b = 0.75;
            let avg_doc_len = 50.0; // Average from generate_documents
            let doc_len = doc_words.len() as f64;
            let normalized_tf =
                term_freq * (k1 + 1.0) / (term_freq + k1 * (1.0 - b + b * doc_len / avg_doc_len));
            score += normalized_tf;
        }
    }
    score
}

/// Generate expected results based on distance metric and fusion method.
///
/// # Panics
///
/// Panics if score comparison yields NaN (all scores should be finite).
#[must_use]
pub fn generate_expected_results(
    docs: &[TestDocument],
    query: &TestQuery,
    limit: usize,
    alpha: f64,
) -> Vec<ExpectedResult> {
    let mut scored_docs: Vec<_> = docs
        .iter()
        .map(|doc| {
            let bm25 = simple_bm25_score(&doc.content, &query.text);
            let distance = l2_distance(&doc.embedding, &query.embedding);
            let vector_score = 1.0 / (1.0 + distance); // Normalize to similarity
            let combined = alpha * bm25 / (bm25 + 1.0) + (1.0 - alpha) * vector_score;
            (doc.id, combined)
        })
        .collect();

    scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    scored_docs
        .iter()
        .take(limit)
        .enumerate()
        .map(|(rank, (doc_id, score))| ExpectedResult {
            doc_id: *doc_id,
            expected_rank_range: (rank, rank + 5), // Allow some variance
            min_score: score * 0.9,                // Allow 10% score variance
        })
        .collect()
}

/// Generate a large dataset for performance testing.
#[must_use]
pub fn generate_large_dataset(size: usize, embedding_dim: usize) -> Vec<TestDocument> {
    generate_documents(size, embedding_dim, 12345)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_documents() {
        let docs = generate_documents(100, 128, 42);
        assert_eq!(docs.len(), 100);
        assert_eq!(docs[0].embedding.len(), 128);
        assert!(docs.iter().all(|d| !d.title.is_empty()));
        assert!(docs.iter().all(|d| !d.content.is_empty()));
    }

    #[test]
    fn test_generate_high_fts_selectivity_query() {
        let query = generate_high_fts_selectivity_query(64);
        assert!(query.expected_fts_selectivity < 0.01);
        assert_eq!(query.embedding.len(), 64);
    }

    #[test]
    fn test_generate_high_vector_selectivity_query() {
        let query = generate_high_vector_selectivity_query(64);
        assert!(query.expected_vector_selectivity < 0.01);
        assert_eq!(query.embedding.len(), 64);
    }

    #[test]
    fn test_l2_distance() {
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((l2_distance(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);

        let c = vec![1.0, 0.0, 0.0];
        let d = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&c, &d).abs() < 1e-6);
    }

    #[test]
    fn test_inner_product() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        assert!((inner_product(&a, &b) - 32.0).abs() < 1e-6);
    }

    #[test]
    fn test_simple_bm25_score() {
        let doc = "machine learning algorithm data neural network";
        let query = "machine learning";
        let score = simple_bm25_score(doc, query);
        assert!(score > 0.0);

        let doc2 = "unrelated content without query terms";
        let score2 = simple_bm25_score(doc2, query);
        assert_eq!(score2, 0.0);
    }

    #[test]
    fn test_generate_expected_results() {
        let docs = generate_documents(10, 32, 99);
        let query = generate_balanced_query(32);
        let results = generate_expected_results(&docs, &query, 5, 0.5);

        assert_eq!(results.len(), 5);
        assert!(results.iter().all(|r| r.min_score >= 0.0));
    }

    #[test]
    fn test_deterministic_generation() {
        let docs1 = generate_documents(50, 64, 12345);
        let docs2 = generate_documents(50, 64, 12345);

        assert_eq!(docs1.len(), docs2.len());
        assert_eq!(docs1[0].title, docs2[0].title);
        assert_eq!(docs1[0].embedding, docs2[0].embedding);
    }

    #[test]
    fn test_varied_queries_coverage() {
        let queries = generate_varied_queries(128);
        assert!(queries.len() >= 5);

        // Check we have at least one highly selective FTS query
        assert!(queries.iter().any(|q| q.expected_fts_selectivity < 0.01));

        // Check we have at least one highly selective vector query
        assert!(queries.iter().any(|q| q.expected_vector_selectivity < 0.01));

        // Check we have at least one balanced query
        assert!(queries.iter().any(|q| {
            (q.expected_fts_selectivity - q.expected_vector_selectivity).abs() < 0.05
        }));
    }
}

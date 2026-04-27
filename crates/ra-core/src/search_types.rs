//! Search-specific types for hybrid search capabilities.
//!
//! Defines distance metrics for vector search, full-text parsers,
//! and ranking algorithms used across different database systems.

use serde::{Deserialize, Serialize};

/// Distance metric for vector similarity search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DistanceMetric {
    /// Euclidean distance (L2 norm).
    L2,
    /// Inner product (dot product).
    InnerProduct,
    /// Cosine similarity.
    Cosine,
    /// Manhattan distance (L1 norm).
    L1,
    /// Hamming distance for binary vectors.
    Hamming,
}

impl DistanceMetric {
    /// Returns the default distance metric for vector search.
    #[must_use]
    pub const fn default_metric() -> Self {
        Self::L2
    }

    /// Returns whether this metric requires normalized vectors.
    #[must_use]
    pub const fn requires_normalization(&self) -> bool {
        matches!(self, Self::Cosine)
    }

    /// Returns whether this metric works with binary vectors.
    #[must_use]
    pub const fn supports_binary(&self) -> bool {
        matches!(self, Self::Hamming | Self::InnerProduct)
    }
}

/// Full-text search parser/tokenizer configuration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FullTextParser {
    /// Standard word-based tokenizer.
    Standard,
    /// N-gram tokenizer with specified size.
    NGram {
        /// Size of n-grams to generate (e.g., 2 for bigrams, 3 for trigrams).
        size: u32,
    },
    /// Trigram (3-gram) tokenizer.
    Trigram,
    /// Porter stemming algorithm.
    Porter,
    /// Unicode-aware tokenizer.
    Unicode,
    /// Whitespace-only tokenizer.
    Whitespace,
    /// Custom parser by name.
    Custom {
        /// Name of the custom parser implementation to use.
        name: String,
    },
}

impl FullTextParser {
    /// Returns the default parser for full-text search.
    #[must_use]
    pub fn default_parser() -> Self {
        Self::Standard
    }
}

/// Ranking algorithm for full-text search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RankingAlgorithm {
    /// BM25 (Best Matching 25) ranking.
    BM25,
    /// Term frequency-inverse document frequency.
    TfIdf,
    /// Okapi BM25 variant.
    OkapiBM25,
    /// Simple term frequency.
    TermFrequency,
    /// Proximity-based ranking.
    Proximity,
    /// `PostgreSQL` text search ranking.
    TSRank,
}

impl RankingAlgorithm {
    /// Returns the default ranking algorithm.
    #[must_use]
    pub const fn default_ranking() -> Self {
        Self::BM25
    }

    /// Returns whether this algorithm considers term proximity.
    #[must_use]
    pub const fn uses_proximity(&self) -> bool {
        matches!(self, Self::Proximity)
    }

    /// Returns whether this algorithm uses document frequency statistics.
    #[must_use]
    pub const fn uses_idf(&self) -> bool {
        matches!(self, Self::BM25 | Self::TfIdf | Self::OkapiBM25)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_metric_defaults() {
        assert_eq!(DistanceMetric::default_metric(), DistanceMetric::L2);
        assert!(DistanceMetric::Cosine.requires_normalization());
        assert!(!DistanceMetric::L2.requires_normalization());
    }

    #[test]
    fn distance_metric_binary_support() {
        assert!(DistanceMetric::Hamming.supports_binary());
        assert!(DistanceMetric::InnerProduct.supports_binary());
        assert!(!DistanceMetric::L2.supports_binary());
        assert!(!DistanceMetric::Cosine.supports_binary());
    }

    #[test]
    fn fulltext_parser_defaults() {
        assert_eq!(
            FullTextParser::default_parser(),
            FullTextParser::Standard
        );
    }

    #[test]
    fn ranking_algorithm_defaults() {
        assert_eq!(
            RankingAlgorithm::default_ranking(),
            RankingAlgorithm::BM25
        );
        assert!(RankingAlgorithm::Proximity.uses_proximity());
        assert!(!RankingAlgorithm::TermFrequency.uses_proximity());
    }

    #[test]
    fn ranking_algorithm_idf() {
        assert!(RankingAlgorithm::BM25.uses_idf());
        assert!(RankingAlgorithm::TfIdf.uses_idf());
        assert!(RankingAlgorithm::OkapiBM25.uses_idf());
        assert!(!RankingAlgorithm::TermFrequency.uses_idf());
        assert!(!RankingAlgorithm::Proximity.uses_idf());
    }

    #[test]
    fn serialize_distance_metric() {
        let metric = DistanceMetric::Cosine;
        let json = serde_json::to_string(&metric).expect("serialize");
        let back: DistanceMetric =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(metric, back);
    }

    #[test]
    fn serialize_fulltext_parser() {
        let parser = FullTextParser::NGram { size: 3 };
        let json = serde_json::to_string(&parser).expect("serialize");
        let back: FullTextParser =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parser, back);
    }

    #[test]
    fn serialize_ranking_algorithm() {
        let algo = RankingAlgorithm::BM25;
        let json = serde_json::to_string(&algo).expect("serialize");
        let back: RankingAlgorithm =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(algo, back);
    }

    #[test]
    fn fulltext_parser_custom() {
        let parser = FullTextParser::Custom {
            name: "my_parser".to_string(),
        };
        assert!(matches!(parser, FullTextParser::Custom { ref name } if name == "my_parser"));
    }

    #[test]
    fn distance_metrics_are_comparable() {
        let l2 = DistanceMetric::L2;
        let cosine = DistanceMetric::Cosine;
        assert_ne!(l2, cosine);
        assert_eq!(l2, DistanceMetric::L2);
    }

    #[test]
    fn ranking_algorithms_are_comparable() {
        let bm25 = RankingAlgorithm::BM25;
        let tfidf = RankingAlgorithm::TfIdf;
        assert_ne!(bm25, tfidf);
        assert_eq!(bm25, RankingAlgorithm::BM25);
    }
}

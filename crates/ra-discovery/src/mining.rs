//! Frequent pattern mining on operator sequences.
//!
//! Implements an FP-Growth-inspired algorithm that discovers
//! frequently occurring subsequences in plan fingerprints.  These
//! frequent patterns are the raw material from which candidate
//! rewrite rules are synthesized.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::fingerprint::{Fingerprint, Token};

/// A frequent pattern: a token subsequence together with the number
/// of fingerprints in which it appears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrequentPattern {
    /// The token sequence.
    pub tokens: Vec<Token>,
    /// Number of fingerprints containing this pattern.
    pub support: usize,
}

/// Configuration for the mining algorithm.
#[derive(Debug, Clone)]
pub struct MiningConfig {
    /// Minimum support count: a pattern must appear in at least
    /// this many fingerprints to be considered frequent.
    pub min_support: usize,
    /// Maximum pattern length (number of tokens).
    pub max_length: usize,
    /// Minimum pattern length (number of tokens).
    pub min_length: usize,
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            min_support: 2,
            max_length: 10,
            min_length: 2,
        }
    }
}

/// Mine frequent patterns from a collection of fingerprints.
///
/// Uses n-gram counting with Apriori-style pruning: only patterns
/// whose sub-patterns are also frequent get extended.
#[must_use]
pub fn mine_frequent_patterns(
    fingerprints: &[Fingerprint],
    config: &MiningConfig,
) -> Vec<FrequentPattern> {
    let mut results = Vec::new();

    for length in config.min_length..=config.max_length {
        let counts = count_ngrams(fingerprints, length);
        let frequent: Vec<FrequentPattern> = counts
            .into_iter()
            .filter(|(_, count)| *count >= config.min_support)
            .map(|(tokens, support)| FrequentPattern { tokens, support })
            .collect();

        if frequent.is_empty() {
            break;
        }
        results.extend(frequent);
    }

    results.sort_by(|a, b| b.support.cmp(&a.support));
    results
}

/// Count occurrences of all n-grams of the given length across
/// all fingerprints.
fn count_ngrams(fingerprints: &[Fingerprint], n: usize) -> HashMap<Vec<Token>, usize> {
    let mut counts: HashMap<Vec<Token>, usize> = HashMap::new();

    for fp in fingerprints {
        let mut seen: HashMap<Vec<Token>, bool> = HashMap::new();
        for window in fp.ngrams(n) {
            let key = window.to_vec();
            if !seen.contains_key(&key) {
                seen.insert(key.clone(), true);
                *counts.entry(key).or_insert(0) += 1;
            }
        }
    }

    counts
}

/// A pair of patterns that appear together frequently, suggesting
/// a potential transformation relationship.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternPair {
    /// Pattern found in slower/original plans.
    pub from_pattern: Vec<Token>,
    /// Pattern found in faster/optimized plans.
    pub to_pattern: Vec<Token>,
    /// Number of log entries where this pair was observed.
    pub co_occurrence: usize,
    /// Average speedup ratio (`original_time / optimized_time`).
    pub avg_speedup: f64,
}

/// Discover pattern pairs by comparing original vs. optimized plans.
///
/// Looks for patterns that appear in original plans but not in
/// optimized plans (and vice versa), suggesting the optimizer
/// transformed one into the other.
#[must_use]
pub fn discover_pattern_pairs(
    original_fps: &[Fingerprint],
    optimized_fps: &[Fingerprint],
    config: &MiningConfig,
) -> Vec<PatternPair> {
    let orig_patterns = mine_frequent_patterns(original_fps, config);
    let opt_patterns = mine_frequent_patterns(optimized_fps, config);

    let orig_set: HashMap<Vec<Token>, usize> = orig_patterns
        .into_iter()
        .map(|p| (p.tokens, p.support))
        .collect();
    let opt_set: HashMap<Vec<Token>, usize> = opt_patterns
        .into_iter()
        .map(|p| (p.tokens, p.support))
        .collect();

    let mut pairs = Vec::new();

    for (from_tokens, from_support) in &orig_set {
        if opt_set.contains_key(from_tokens) {
            continue;
        }
        for (to_tokens, to_support) in &opt_set {
            if orig_set.contains_key(to_tokens) {
                continue;
            }
            let co_occurrence = (*from_support).min(*to_support);
            if co_occurrence >= config.min_support {
                pairs.push(PatternPair {
                    from_pattern: from_tokens.clone(),
                    to_pattern: to_tokens.clone(),
                    co_occurrence,
                    avg_speedup: 1.0,
                });
            }
        }
    }

    pairs.sort_by(|a, b| b.co_occurrence.cmp(&a.co_occurrence));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::Token;

    fn make_fp(tokens: Vec<Token>) -> Fingerprint {
        Fingerprint { tokens }
    }

    #[test]
    fn mine_simple_patterns() {
        let fps = vec![
            make_fp(vec![Token::Filter, Token::Eq, Token::Scan, Token::End]),
            make_fp(vec![Token::Filter, Token::Eq, Token::Scan, Token::End]),
            make_fp(vec![Token::Scan]),
        ];

        let config = MiningConfig {
            min_support: 2,
            max_length: 4,
            min_length: 2,
        };
        let patterns = mine_frequent_patterns(&fps, &config);
        assert!(!patterns.is_empty());

        let filter_eq = patterns
            .iter()
            .find(|p| p.tokens == vec![Token::Filter, Token::Eq]);
        assert!(filter_eq.is_some());
        assert_eq!(filter_eq.map_or(0, |p| p.support), 2);
    }

    #[test]
    fn min_support_filters() {
        let fps = vec![
            make_fp(vec![Token::Filter, Token::Scan, Token::End]),
            make_fp(vec![Token::Scan]),
        ];

        let config = MiningConfig {
            min_support: 2,
            max_length: 3,
            min_length: 2,
        };
        let patterns = mine_frequent_patterns(&fps, &config);
        assert!(patterns.is_empty(), "no pattern appears in 2+ fingerprints");
    }

    #[test]
    fn empty_input() {
        let config = MiningConfig::default();
        let patterns = mine_frequent_patterns(&[], &config);
        assert!(patterns.is_empty());
    }

    #[test]
    fn pattern_pairs_discovery() {
        let orig = vec![
            make_fp(vec![
                Token::Filter,
                Token::Eq,
                Token::Join("INNER".into()),
                Token::Scan,
                Token::Scan,
                Token::End,
                Token::End,
            ]),
            make_fp(vec![
                Token::Filter,
                Token::Eq,
                Token::Join("INNER".into()),
                Token::Scan,
                Token::Scan,
                Token::End,
                Token::End,
            ]),
        ];

        let opt = vec![
            make_fp(vec![
                Token::Join("INNER".into()),
                Token::Eq,
                Token::Filter,
                Token::Eq,
                Token::Scan,
                Token::End,
                Token::Scan,
                Token::End,
            ]),
            make_fp(vec![
                Token::Join("INNER".into()),
                Token::Eq,
                Token::Filter,
                Token::Eq,
                Token::Scan,
                Token::End,
                Token::Scan,
                Token::End,
            ]),
        ];

        let config = MiningConfig {
            min_support: 2,
            max_length: 4,
            min_length: 2,
        };

        let pairs = discover_pattern_pairs(&orig, &opt, &config);
        assert!(!pairs.is_empty(), "should find transformation pairs");
    }

    #[test]
    fn sorted_by_support_descending() {
        let fps = vec![
            make_fp(vec![Token::Scan, Token::End, Token::Filter]),
            make_fp(vec![Token::Scan, Token::End, Token::Filter]),
            make_fp(vec![Token::Scan, Token::End, Token::Filter]),
            make_fp(vec![Token::Filter, Token::Gt, Token::Scan]),
            make_fp(vec![Token::Filter, Token::Gt, Token::Scan]),
        ];

        let config = MiningConfig {
            min_support: 2,
            max_length: 3,
            min_length: 2,
        };
        let patterns = mine_frequent_patterns(&fps, &config);

        for i in 1..patterns.len() {
            assert!(patterns[i - 1].support >= patterns[i].support);
        }
    }
}

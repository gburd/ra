//! Rule synthesis from mined patterns.
//!
//! Takes frequent pattern pairs (from -> to transformations observed
//! in execution logs) and synthesizes candidate [`ra_core::rule::Rule`]
//! implementations.  Candidate rules are structural rewrites
//! expressed as pattern -> replacement pairs.

use serde::{Deserialize, Serialize};

use ra_core::algebra::RelExpr;
use ra_core::pattern::Pattern;
use ra_core::rule::{RuleCategory, RuleMetadata};

use crate::fingerprint::Token;
use crate::mining::PatternPair;

/// A candidate rewrite rule discovered from execution logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateRule {
    /// Metadata describing the candidate rule.
    pub metadata: RuleMetadata,
    /// The structural pattern that the rule matches against.
    pub match_pattern: StructuralPattern,
    /// The replacement pattern to apply.
    pub replacement_pattern: StructuralPattern,
    /// Confidence score in [0.0, 1.0] based on how often the
    /// transformation was observed to improve performance.
    pub confidence: f64,
    /// Number of log entries that support this rule.
    pub support: usize,
    /// Average speedup observed in the training data.
    pub avg_speedup: f64,
}

/// A structural pattern described as a tree shape built from tokens.
///
/// Unlike `ra_core::pattern::Pattern`, this is learned from data and
/// may represent partial patterns that need further refinement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralPattern {
    /// The root operator token.
    pub root: Token,
    /// Child patterns (may be empty for leaf nodes).
    pub children: Vec<StructuralPattern>,
}

impl StructuralPattern {
    /// Attempt to convert to a core `Pattern` for use in the
    /// optimizer.  Returns `None` if the structural pattern cannot
    /// be represented.
    #[must_use]
    pub fn to_core_pattern(&self) -> Option<Pattern> {
        match &self.root {
            Token::Scan => Some(Pattern::Scan { table: None }),
            Token::Filter => {
                let input = self
                    .children
                    .iter()
                    .find(|c| is_relational_token(&c.root))?
                    .to_core_pattern()?;
                Some(Pattern::Filter {
                    predicate: None,
                    input: Box::new(input),
                })
            }
            Token::Project => {
                let input = self.children.first()?.to_core_pattern()?;
                Some(Pattern::Project {
                    input: Box::new(input),
                })
            }
            Token::Join(_) => {
                let rel_children: Vec<&StructuralPattern> = self
                    .children
                    .iter()
                    .filter(|c| is_relational_token(&c.root))
                    .collect();
                if rel_children.len() < 2 {
                    return None;
                }
                let left = rel_children[0].to_core_pattern()?;
                let right = rel_children[1].to_core_pattern()?;
                Some(Pattern::Join {
                    join_type: None,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            Token::Aggregate => {
                let input = self.children.first()?.to_core_pattern()?;
                Some(Pattern::Aggregate {
                    input: Box::new(input),
                })
            }
            Token::Sort => {
                let input = self.children.first()?.to_core_pattern()?;
                Some(Pattern::Sort {
                    input: Box::new(input),
                })
            }
            Token::Limit => {
                let input = self.children.first()?.to_core_pattern()?;
                Some(Pattern::Limit {
                    input: Box::new(input),
                })
            }
            _ => Some(Pattern::wildcard("_")),
        }
    }

    /// Check if this pattern would match the given expression.
    #[must_use]
    pub fn matches(&self, expr: &RelExpr) -> bool {
        match (&self.root, expr) {
            (Token::Scan, RelExpr::Scan { .. }) => true,
            (Token::Filter, RelExpr::Filter { input, .. }) => self
                .children
                .iter()
                .filter(|c| is_relational_token(&c.root))
                .all(|child| child.matches(input)),
            (Token::Project, RelExpr::Project { input, .. }) => self
                .children
                .first()
                .map_or(true, |child| child.matches(input)),
            (Token::Join(_), RelExpr::Join { left, right, .. }) => {
                let rel_children: Vec<&StructuralPattern> = self
                    .children
                    .iter()
                    .filter(|c| is_relational_token(&c.root))
                    .collect();
                match rel_children.len() {
                    0 => true,
                    1 => rel_children[0].matches(left),
                    _ => rel_children[0].matches(left) && rel_children[1].matches(right),
                }
            }
            (Token::Aggregate, RelExpr::Aggregate { input, .. }) => self
                .children
                .first()
                .map_or(true, |child| child.matches(input)),
            _ => false,
        }
    }
}

fn is_relational_token(token: &Token) -> bool {
    matches!(
        token,
        Token::Scan
            | Token::Filter
            | Token::Project
            | Token::Join(_)
            | Token::Aggregate
            | Token::Sort
            | Token::Limit
            | Token::Union
            | Token::Intersect
            | Token::Except
    )
}

/// Configuration for rule synthesis.
#[derive(Debug, Clone)]
pub struct SynthesisConfig {
    /// Minimum confidence to accept a candidate rule.
    pub min_confidence: f64,
    /// Minimum support (number of log entries).
    pub min_support: usize,
    /// Minimum average speedup to consider the rule beneficial.
    pub min_speedup: f64,
}

impl Default for SynthesisConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            min_support: 3,
            min_speedup: 1.05,
        }
    }
}

/// Synthesize candidate rules from mined pattern pairs.
///
/// Each pattern pair is converted into a structural match pattern
/// and replacement pattern.  Rules below the confidence or support
/// thresholds are discarded.
#[must_use]
pub fn synthesize_rules(pairs: &[PatternPair], config: &SynthesisConfig) -> Vec<CandidateRule> {
    let mut candidates = Vec::new();

    for (idx, pair) in pairs.iter().enumerate() {
        if pair.co_occurrence < config.min_support {
            continue;
        }
        if pair.avg_speedup < config.min_speedup {
            continue;
        }

        let match_pattern = tokens_to_structural(&pair.from_pattern);
        let replacement_pattern = tokens_to_structural(&pair.to_pattern);

        if let (Some(mp), Some(rp)) = (match_pattern, replacement_pattern) {
            let confidence = compute_confidence(pair);
            if confidence < config.min_confidence {
                continue;
            }

            candidates.push(CandidateRule {
                metadata: RuleMetadata {
                    id: format!("discovered-{idx}"),
                    name: format!("Discovered rule #{idx}"),
                    description: format!(
                        "Auto-discovered from {} observations \
                         with {:.1}x avg speedup",
                        pair.co_occurrence, pair.avg_speedup
                    ),
                    category: RuleCategory::Logical,
                    databases: vec![],
                    priority: 100,
                    preconditions: vec![],
                },
                match_pattern: mp,
                replacement_pattern: rp,
                confidence,
                support: pair.co_occurrence,
                avg_speedup: pair.avg_speedup,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

fn compute_confidence(pair: &PatternPair) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let support_factor = (pair.co_occurrence as f64 / 10.0).min(1.0);
    let speedup_factor = if pair.avg_speedup > 1.0 {
        1.0 - (1.0 / pair.avg_speedup)
    } else {
        0.0
    };
    (support_factor + speedup_factor) / 2.0
}

/// Convert a flat token sequence into a structural pattern tree.
///
/// Returns `None` if the sequence is empty or cannot be parsed
/// into a tree.
fn tokens_to_structural(tokens: &[Token]) -> Option<StructuralPattern> {
    if tokens.is_empty() {
        return None;
    }

    let (pattern, _) = parse_tokens(tokens, 0)?;
    Some(pattern)
}

fn parse_tokens(tokens: &[Token], pos: usize) -> Option<(StructuralPattern, usize)> {
    if pos >= tokens.len() {
        return None;
    }

    let root = tokens[pos].clone();
    let mut children = Vec::new();
    let mut current = pos + 1;

    match &root {
        Token::End
        | Token::Eq
        | Token::Lt
        | Token::Gt
        | Token::And
        | Token::Or
        | Token::Expr
        | Token::Scan => {
            return Some((StructuralPattern { root, children }, current));
        }
        _ => {}
    }

    while current < tokens.len() {
        if tokens[current] == Token::End {
            current += 1;
            break;
        }
        if let Some((child, next)) = parse_tokens(tokens, current) {
            children.push(child);
            current = next;
        } else {
            break;
        }
    }

    Some((StructuralPattern { root, children }, current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::Token;
    use crate::mining::PatternPair;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn tokens_to_structural_scan() {
        let sp = tokens_to_structural(&[Token::Scan]);
        assert!(sp.is_some());
        let sp = sp.unwrap_or_else(|| unreachable!());
        assert_eq!(sp.root, Token::Scan);
        assert!(sp.children.is_empty());
    }

    #[test]
    fn tokens_to_structural_filter() {
        let sp = tokens_to_structural(&[Token::Filter, Token::Eq, Token::Scan, Token::End]);
        assert!(sp.is_some());
        let sp = sp.unwrap_or_else(|| unreachable!());
        assert_eq!(sp.root, Token::Filter);
        assert_eq!(sp.children.len(), 2);
    }

    #[test]
    fn structural_matches_scan() {
        let sp = StructuralPattern {
            root: Token::Scan,
            children: vec![],
        };
        assert!(sp.matches(&RelExpr::scan("users")));
        assert!(!sp.matches(&RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)))));
    }

    #[test]
    fn structural_matches_filter() {
        let sp = StructuralPattern {
            root: Token::Filter,
            children: vec![StructuralPattern {
                root: Token::Scan,
                children: vec![],
            }],
        };
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });
        assert!(sp.matches(&expr));
    }

    #[test]
    fn structural_matches_join() {
        let sp = StructuralPattern {
            root: Token::Join("INNER".into()),
            children: vec![
                StructuralPattern {
                    root: Token::Scan,
                    children: vec![],
                },
                StructuralPattern {
                    root: Token::Scan,
                    children: vec![],
                },
            ],
        };
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("a")),
            right: Box::new(RelExpr::scan("b")),
        };
        assert!(sp.matches(&expr));
    }

    #[test]
    fn to_core_pattern_scan() {
        let sp = StructuralPattern {
            root: Token::Scan,
            children: vec![],
        };
        let core = sp.to_core_pattern();
        assert!(core.is_some());
    }

    #[test]
    fn synthesize_filters_by_config() {
        let pairs = vec![PatternPair {
            from_pattern: vec![Token::Filter, Token::Scan, Token::End],
            to_pattern: vec![Token::Scan],
            co_occurrence: 10,
            avg_speedup: 2.0,
        }];

        let config = SynthesisConfig {
            min_confidence: 0.0,
            min_support: 1,
            min_speedup: 1.0,
        };

        let rules = synthesize_rules(&pairs, &config);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].support, 10);
    }

    #[test]
    fn synthesize_rejects_low_support() {
        let pairs = vec![PatternPair {
            from_pattern: vec![Token::Filter, Token::Scan, Token::End],
            to_pattern: vec![Token::Scan],
            co_occurrence: 1,
            avg_speedup: 2.0,
        }];

        let config = SynthesisConfig {
            min_confidence: 0.0,
            min_support: 5,
            min_speedup: 1.0,
        };

        let rules = synthesize_rules(&pairs, &config);
        assert!(rules.is_empty());
    }

    #[test]
    fn synthesize_rejects_low_speedup() {
        let pairs = vec![PatternPair {
            from_pattern: vec![Token::Filter, Token::Scan, Token::End],
            to_pattern: vec![Token::Scan],
            co_occurrence: 10,
            avg_speedup: 0.9,
        }];

        let config = SynthesisConfig {
            min_confidence: 0.0,
            min_support: 1,
            min_speedup: 1.05,
        };

        let rules = synthesize_rules(&pairs, &config);
        assert!(rules.is_empty());
    }

    #[test]
    fn confidence_computation() {
        let pair = PatternPair {
            from_pattern: vec![],
            to_pattern: vec![],
            co_occurrence: 10,
            avg_speedup: 2.0,
        };
        let c = compute_confidence(&pair);
        assert!(c > 0.0);
        assert!(c <= 1.0);
    }

    #[test]
    fn confidence_zero_for_no_speedup() {
        let pair = PatternPair {
            from_pattern: vec![],
            to_pattern: vec![],
            co_occurrence: 10,
            avg_speedup: 0.5,
        };
        let c = compute_confidence(&pair);
        assert!((c - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_pairs_no_rules() {
        let rules = synthesize_rules(&[], &SynthesisConfig::default());
        assert!(rules.is_empty());
    }
}

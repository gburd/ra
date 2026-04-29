#![allow(clippy::doc_markdown)]
//! Row Pattern Recognition (RPR) types for SQL:2016 MATCH_RECOGNIZE.
//!
//! This module defines the core algebra types for row pattern
//! recognition: pattern expressions, quantifiers, variable
//! definitions, and measure computations.

use serde::{Deserialize, Serialize};

use crate::expr::Expr;

/// A pattern expression in `MATCH_RECOGNIZE` PATTERN clause.
///
/// Represents the regex-like pattern over row variables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PatternExpr {
    /// A single pattern variable reference (e.g., `A`).
    Var(String),

    /// A sequence of patterns (e.g., `A B C`).
    Sequence(Vec<PatternExpr>),

    /// An alternation of patterns (e.g., `A | B`).
    Alternation(Vec<PatternExpr>),

    /// A quantified pattern (e.g., `A+`, `B*`, `C{2,5}`).
    Quantified(Box<PatternExpr>, Quantifier),

    /// A grouped sub-pattern (e.g., `(A B)+`).
    Group(Box<PatternExpr>),
}

/// Quantifiers for pattern expressions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quantifier {
    /// Matches zero or one occurrence (`?`).
    ZeroOrOne,

    /// Matches zero or more occurrences (`*`).
    ZeroOrMore,

    /// Matches one or more occurrences (`+`).
    OneOrMore,

    /// Matches exactly `n` occurrences (`{n}`).
    Exactly(usize),

    /// Matches between `min` and optional `max` occurrences.
    /// `{n,m}` or `{n,}` when max is `None`.
    Range(usize, Option<usize>),
}

/// A variable definition in the DEFINE clause.
///
/// Maps a pattern variable name to a boolean condition
/// that rows must satisfy to match that variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternDefine {
    /// The pattern variable name (e.g., "A", "B").
    pub variable: String,

    /// The boolean condition rows must satisfy.
    pub condition: Expr,
}

/// A measure computation in the MEASURES clause.
///
/// Expressions computed over matched row sequences,
/// output as result columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternMeasure {
    /// The expression to compute (may use navigation functions).
    pub expr: Expr,

    /// The output column alias.
    pub alias: String,
}

/// Match mode: how many rows to output per match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchMode {
    /// Output one row per match.
    OneRowPerMatch,

    /// Output all rows per match.
    AllRowsPerMatch,

    /// Include unmatched rows in output.
    AllRowsPerMatchWithUnmatched,
}

/// Skip mode: where to resume after a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkipMode {
    /// Resume after the last row of the match.
    PastLastRow,

    /// Resume at the next row after the start of the match.
    ToNextRow,

    /// Resume at the first row matching a specific variable.
    ToFirst(/* variable index placeholder */ usize),

    /// Resume at the last row matching a specific variable.
    ToLast(/* variable index placeholder */ usize),
}

impl PatternExpr {
    /// Collect all variable names referenced in this pattern.
    #[must_use]
    pub fn variables(&self) -> Vec<&str> {
        let mut vars = Vec::new();
        self.collect_variables(&mut vars);
        vars
    }

    fn collect_variables<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Var(name) => out.push(name),
            Self::Sequence(parts) | Self::Alternation(parts) => {
                for part in parts {
                    part.collect_variables(out);
                }
            }
            Self::Quantified(inner, _) | Self::Group(inner) => {
                inner.collect_variables(out);
            }
        }
    }

    /// Estimate the number of DFA states for this pattern.
    #[must_use]
    pub fn estimate_dfa_states(&self) -> usize {
        match self {
            Self::Var(_) => 2,
            Self::Sequence(parts) => parts.iter().map(Self::estimate_dfa_states).sum(),
            Self::Alternation(branches) => branches
                .iter()
                .map(Self::estimate_dfa_states)
                .max()
                .unwrap_or(2),
            Self::Quantified(inner, quant) => {
                let base = inner.estimate_dfa_states();
                match quant {
                    Quantifier::ZeroOrOne => base + 1,
                    Quantifier::ZeroOrMore | Quantifier::OneOrMore => base * 2,
                    Quantifier::Exactly(n) => base * n,
                    Quantifier::Range(min, max) => base * max.unwrap_or(min + 10),
                }
            }
            Self::Group(inner) => inner.estimate_dfa_states(),
        }
    }

    /// Check whether this pattern is empty (no variables).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Var(_) => false,
            Self::Sequence(parts) | Self::Alternation(parts) => parts.is_empty(),
            Self::Quantified(inner, _) | Self::Group(inner) => inner.is_empty(),
        }
    }
}

impl std::fmt::Display for PatternExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Var(name) => write!(f, "{name}"),
            Self::Sequence(parts) => {
                let strs: Vec<String> = parts.iter().map(ToString::to_string).collect();
                write!(f, "{}", strs.join(" "))
            }
            Self::Alternation(branches) => {
                let strs: Vec<String> = branches.iter().map(ToString::to_string).collect();
                write!(f, "{}", strs.join(" | "))
            }
            Self::Quantified(inner, quant) => {
                write!(f, "{inner}{quant}")
            }
            Self::Group(inner) => write!(f, "({inner})"),
        }
    }
}

impl std::fmt::Display for Quantifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroOrOne => write!(f, "?"),
            Self::ZeroOrMore => write!(f, "*"),
            Self::OneOrMore => write!(f, "+"),
            Self::Exactly(n) => write!(f, "{{{n}}}"),
            Self::Range(min, Some(max)) => {
                write!(f, "{{{min},{max}}}")
            }
            Self::Range(min, None) => write!(f, "{{{min},}}"),
        }
    }
}

impl std::fmt::Display for MatchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OneRowPerMatch => write!(f, "ONE ROW PER MATCH"),
            Self::AllRowsPerMatch => {
                write!(f, "ALL ROWS PER MATCH")
            }
            Self::AllRowsPerMatchWithUnmatched => {
                write!(f, "ALL ROWS PER MATCH WITH UNMATCHED ROWS")
            }
        }
    }
}

impl std::fmt::Display for SkipMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PastLastRow => {
                write!(f, "SKIP PAST LAST ROW")
            }
            Self::ToNextRow => write!(f, "SKIP TO NEXT ROW"),
            Self::ToFirst(idx) => {
                write!(f, "SKIP TO FIRST[{idx}]")
            }
            Self::ToLast(idx) => {
                write!(f, "SKIP TO LAST[{idx}]")
            }
        }
    }
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{BinOp, ColumnRef, Const};

    #[test]
    fn pattern_var_display() {
        let p = PatternExpr::Var("A".to_owned());
        assert_eq!(p.to_string(), "A");
    }

    #[test]
    fn pattern_sequence_display() {
        let p = PatternExpr::Sequence(vec![
            PatternExpr::Var("A".to_owned()),
            PatternExpr::Var("B".to_owned()),
            PatternExpr::Var("C".to_owned()),
        ]);
        assert_eq!(p.to_string(), "A B C");
    }

    #[test]
    fn pattern_alternation_display() {
        let p = PatternExpr::Alternation(vec![
            PatternExpr::Var("A".to_owned()),
            PatternExpr::Var("B".to_owned()),
        ]);
        assert_eq!(p.to_string(), "A | B");
    }

    #[test]
    fn pattern_quantified_display() {
        let p = PatternExpr::Quantified(
            Box::new(PatternExpr::Var("A".to_owned())),
            Quantifier::OneOrMore,
        );
        assert_eq!(p.to_string(), "A+");
    }

    #[test]
    fn pattern_group_quantified_display() {
        let p = PatternExpr::Quantified(
            Box::new(PatternExpr::Group(Box::new(PatternExpr::Sequence(vec![
                PatternExpr::Var("A".to_owned()),
                PatternExpr::Var("B".to_owned()),
            ])))),
            Quantifier::ZeroOrMore,
        );
        assert_eq!(p.to_string(), "(A B)*");
    }

    #[test]
    fn quantifier_exactly_display() {
        assert_eq!(Quantifier::Exactly(3).to_string(), "{3}");
    }

    #[test]
    fn quantifier_range_display() {
        assert_eq!(Quantifier::Range(2, Some(5)).to_string(), "{2,5}");
        assert_eq!(Quantifier::Range(2, None).to_string(), "{2,}");
    }

    #[test]
    fn pattern_variables_simple() {
        let p = PatternExpr::Sequence(vec![
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::OneOrMore,
            ),
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("B".to_owned())),
                Quantifier::Range(2, Some(5)),
            ),
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("C".to_owned())),
                Quantifier::OneOrMore,
            ),
        ]);
        let vars = p.variables();
        assert_eq!(vars, vec!["A", "B", "C"]);
    }

    #[test]
    fn pattern_variables_alternation() {
        let p = PatternExpr::Alternation(vec![
            PatternExpr::Var("X".to_owned()),
            PatternExpr::Var("Y".to_owned()),
        ]);
        let vars = p.variables();
        assert_eq!(vars, vec!["X", "Y"]);
    }

    #[test]
    fn pattern_variables_nested_group() {
        let p = PatternExpr::Group(Box::new(PatternExpr::Sequence(vec![
            PatternExpr::Var("A".to_owned()),
            PatternExpr::Var("B".to_owned()),
        ])));
        assert_eq!(p.variables(), vec!["A", "B"]);
    }

    #[test]
    fn dfa_state_estimate_var() {
        let p = PatternExpr::Var("A".to_owned());
        assert_eq!(p.estimate_dfa_states(), 2);
    }

    #[test]
    fn dfa_state_estimate_sequence() {
        let p = PatternExpr::Sequence(vec![
            PatternExpr::Var("A".to_owned()),
            PatternExpr::Var("B".to_owned()),
        ]);
        assert_eq!(p.estimate_dfa_states(), 4);
    }

    #[test]
    fn dfa_state_estimate_quantified() {
        let p = PatternExpr::Quantified(
            Box::new(PatternExpr::Var("A".to_owned())),
            Quantifier::OneOrMore,
        );
        assert_eq!(p.estimate_dfa_states(), 4);

        let p2 = PatternExpr::Quantified(
            Box::new(PatternExpr::Var("A".to_owned())),
            Quantifier::Exactly(3),
        );
        assert_eq!(p2.estimate_dfa_states(), 6);
    }

    #[test]
    fn dfa_state_estimate_alternation() {
        let p = PatternExpr::Alternation(vec![
            PatternExpr::Sequence(vec![
                PatternExpr::Var("A".to_owned()),
                PatternExpr::Var("B".to_owned()),
            ]),
            PatternExpr::Var("C".to_owned()),
        ]);
        // max(4, 2) = 4
        assert_eq!(p.estimate_dfa_states(), 4);
    }

    #[test]
    fn pattern_is_empty() {
        let empty = PatternExpr::Sequence(vec![]);
        assert!(empty.is_empty());

        let non_empty = PatternExpr::Var("A".to_owned());
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn pattern_define_construction() {
        let define = PatternDefine {
            variable: "A".to_owned(),
            condition: Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(Expr::Column(ColumnRef::new("price"))),
                right: Box::new(Expr::PatternPrev(
                    Box::new(Expr::Column(ColumnRef::new("price"))),
                    1,
                )),
            },
        };
        assert_eq!(define.variable, "A");
    }

    #[test]
    fn pattern_measure_construction() {
        let measure = PatternMeasure {
            expr: Expr::PatternFirst(
                Box::new(Expr::Column(ColumnRef::new("price"))),
                "A".to_owned(),
            ),
            alias: "start_price".to_owned(),
        };
        assert_eq!(measure.alias, "start_price");
    }

    #[test]
    fn match_mode_display() {
        assert_eq!(MatchMode::OneRowPerMatch.to_string(), "ONE ROW PER MATCH");
        assert_eq!(MatchMode::AllRowsPerMatch.to_string(), "ALL ROWS PER MATCH");
        assert_eq!(
            MatchMode::AllRowsPerMatchWithUnmatched.to_string(),
            "ALL ROWS PER MATCH WITH UNMATCHED ROWS"
        );
    }

    #[test]
    fn skip_mode_display() {
        assert_eq!(SkipMode::PastLastRow.to_string(), "SKIP PAST LAST ROW");
        assert_eq!(SkipMode::ToNextRow.to_string(), "SKIP TO NEXT ROW");
    }

    #[test]
    fn pattern_expr_serialize_roundtrip() {
        let p = PatternExpr::Sequence(vec![
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::OneOrMore,
            ),
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("B".to_owned())),
                Quantifier::Range(2, Some(5)),
            ),
        ]);
        let json = serde_json::to_string(&p).expect("serialization should succeed");
        let deser: PatternExpr =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(p, deser);
    }

    #[test]
    fn quantifier_serialize_roundtrip() {
        let quantifiers = vec![
            Quantifier::ZeroOrOne,
            Quantifier::ZeroOrMore,
            Quantifier::OneOrMore,
            Quantifier::Exactly(5),
            Quantifier::Range(2, Some(10)),
            Quantifier::Range(3, None),
        ];
        for q in &quantifiers {
            let json = serde_json::to_string(q).expect("serialization should succeed");
            let deser: Quantifier =
                serde_json::from_str(&json).expect("deserialization should succeed");
            assert_eq!(*q, deser);
        }
    }

    #[test]
    fn pattern_define_serialize_roundtrip() {
        let define = PatternDefine {
            variable: "A".to_owned(),
            condition: Expr::Const(Const::Bool(true)),
        };
        let json = serde_json::to_string(&define).expect("serialization should succeed");
        let deser: PatternDefine =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(define, deser);
    }

    #[test]
    fn complex_v_pattern() {
        // A+ B{2,5} C+ (V-shape pattern from proposal)
        let p = PatternExpr::Sequence(vec![
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::OneOrMore,
            ),
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("B".to_owned())),
                Quantifier::Range(2, Some(5)),
            ),
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("C".to_owned())),
                Quantifier::OneOrMore,
            ),
        ]);
        assert_eq!(p.to_string(), "A+ B{2,5} C+");
        assert_eq!(p.variables(), vec!["A", "B", "C"]);
        // 4 + 2*5=10 + 4 = 18
        assert_eq!(p.estimate_dfa_states(), 18);
    }
}

//! Plan fingerprinting for pattern mining.
//!
//! Converts relational expression trees into linearized sequences
//! of operator tokens that can be fed into frequent pattern mining
//! algorithms.  Two plans with the same fingerprint are structurally
//! identical (same operator shapes) even if they reference different
//! tables or constants.

use std::fmt;

use serde::{Deserialize, Serialize};

use ra_core::algebra::RelExpr;
use ra_core::expr::Expr;

/// An abstract token representing one operator or expression node.
///
/// Tokens strip away concrete names and constants so that the same
/// structural pattern is recognised across different queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Token {
    /// A table scan.
    Scan,
    /// A filter operator.
    Filter,
    /// A projection.
    Project,
    /// A join with its type as a string tag.
    Join(String),
    /// An aggregate.
    Aggregate,
    /// A sort.
    Sort,
    /// A limit.
    Limit,
    /// A union.
    Union,
    /// An intersect.
    Intersect,
    /// An except.
    Except,
    /// An equality comparison in an expression.
    Eq,
    /// A less-than comparison.
    Lt,
    /// A greater-than comparison.
    Gt,
    /// A logical AND.
    And,
    /// A logical OR.
    Or,
    /// Any other expression node.
    Expr,
    /// Marks the end of a subtree (for prefix encoding).
    End,
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan => write!(f, "SCAN"),
            Self::Filter => write!(f, "FILTER"),
            Self::Project => write!(f, "PROJECT"),
            Self::Join(jt) => write!(f, "JOIN({jt})"),
            Self::Aggregate => write!(f, "AGGREGATE"),
            Self::Sort => write!(f, "SORT"),
            Self::Limit => write!(f, "LIMIT"),
            Self::Union => write!(f, "UNION"),
            Self::Intersect => write!(f, "INTERSECT"),
            Self::Except => write!(f, "EXCEPT"),
            Self::Eq => write!(f, "EQ"),
            Self::Lt => write!(f, "LT"),
            Self::Gt => write!(f, "GT"),
            Self::And => write!(f, "AND"),
            Self::Or => write!(f, "OR"),
            Self::Expr => write!(f, "EXPR"),
            Self::End => write!(f, "END"),
        }
    }
}

/// A fingerprint is a linearised sequence of tokens from a plan tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint {
    /// The token sequence.
    pub tokens: Vec<Token>,
}

impl Fingerprint {
    /// Compute the fingerprint of a relational expression.
    #[must_use]
    pub fn of(expr: &RelExpr) -> Self {
        let mut tokens = Vec::new();
        fingerprint_rel(expr, &mut tokens);
        Self { tokens }
    }

    /// Return the number of tokens.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Check if the fingerprint is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Extract all contiguous subsequences of length `n`.
    #[must_use]
    pub fn ngrams(&self, n: usize) -> Vec<&[Token]> {
        if n == 0 || n > self.tokens.len() {
            return Vec::new();
        }
        self.tokens.windows(n).collect()
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.tokens.iter().map(ToString::to_string).collect();
        write!(f, "{}", parts.join(" "))
    }
}

fn fingerprint_rel(expr: &RelExpr, out: &mut Vec<Token>) {
    match expr {
        RelExpr::Scan { .. } => {
            out.push(Token::Scan);
        }
        RelExpr::Filter {
            predicate, input, ..
        } => {
            out.push(Token::Filter);
            fingerprint_expr(predicate, out);
            fingerprint_rel(input, out);
            out.push(Token::End);
        }
        RelExpr::Project { input, .. } => {
            out.push(Token::Project);
            fingerprint_rel(input, out);
            out.push(Token::End);
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
            ..
        } => {
            out.push(Token::Join(join_type.to_string()));
            fingerprint_expr(condition, out);
            fingerprint_rel(left, out);
            fingerprint_rel(right, out);
            out.push(Token::End);
        }
        RelExpr::Aggregate { input, .. } => {
            out.push(Token::Aggregate);
            fingerprint_rel(input, out);
            out.push(Token::End);
        }
        RelExpr::Sort { input, .. } => {
            out.push(Token::Sort);
            fingerprint_rel(input, out);
            out.push(Token::End);
        }
        RelExpr::Limit { input, .. } => {
            out.push(Token::Limit);
            fingerprint_rel(input, out);
            out.push(Token::End);
        }
        RelExpr::Union { left, right, .. } => {
            out.push(Token::Union);
            fingerprint_rel(left, out);
            fingerprint_rel(right, out);
            out.push(Token::End);
        }
        RelExpr::Intersect { left, right, .. } => {
            out.push(Token::Intersect);
            fingerprint_rel(left, out);
            fingerprint_rel(right, out);
            out.push(Token::End);
        }
        RelExpr::Except { left, right, .. } => {
            out.push(Token::Except);
            fingerprint_rel(left, out);
            fingerprint_rel(right, out);
            out.push(Token::End);
        }
        RelExpr::CTE {
            definition, body, ..
        } => {
            out.push(Token::Aggregate);
            fingerprint_rel(definition, out);
            fingerprint_rel(body, out);
            out.push(Token::End);
        }
        RelExpr::Window { input, .. } => {
            out.push(Token::Project);
            fingerprint_rel(input, out);
            out.push(Token::End);
        }
        RelExpr::Distinct { input, .. } => {
            out.push(Token::Project);
            fingerprint_rel(input, out);
            out.push(Token::End);
        }
        RelExpr::RecursiveCTE {
            base_case,
            recursive_case,
            body,
            ..
        } => {
            out.push(Token::Aggregate);
            fingerprint_rel(base_case, out);
            fingerprint_rel(recursive_case, out);
            fingerprint_rel(body, out);
            out.push(Token::End);
        }
        RelExpr::Values { .. } => {
            out.push(Token::Scan);
        }
    }
}

fn fingerprint_expr(expr: &Expr, out: &mut Vec<Token>) {
    match expr {
        Expr::BinOp { op, .. } => {
            let token = match op {
                ra_core::expr::BinOp::Eq => Token::Eq,
                ra_core::expr::BinOp::Lt => Token::Lt,
                ra_core::expr::BinOp::Gt => Token::Gt,
                ra_core::expr::BinOp::And => Token::And,
                ra_core::expr::BinOp::Or => Token::Or,
                _ => Token::Expr,
            };
            out.push(token);
        }
        _ => {
            out.push(Token::Expr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{JoinType, RelExpr};
    use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

    #[test]
    fn scan_fingerprint() {
        let fp = Fingerprint::of(&RelExpr::scan("users"));
        assert_eq!(fp.tokens, vec![Token::Scan]);
    }

    #[test]
    fn filter_fingerprint() {
        let expr = RelExpr::scan("t").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("a"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });
        let fp = Fingerprint::of(&expr);
        assert_eq!(
            fp.tokens,
            vec![Token::Filter, Token::Eq, Token::Scan, Token::End]
        );
    }

    #[test]
    fn join_fingerprint() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        };
        let fp = Fingerprint::of(&expr);
        assert_eq!(
            fp.tokens,
            vec![
                Token::Join("INNER".into()),
                Token::Eq,
                Token::Scan,
                Token::Scan,
                Token::End,
            ]
        );
    }

    #[test]
    fn same_shape_same_fingerprint() {
        let a = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("id"))),
            right: Box::new(Expr::Const(Const::Int(1))),
        });
        let b = RelExpr::scan("orders").filter(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new("status"))),
            right: Box::new(Expr::Const(Const::String("open".into()))),
        });
        assert_eq!(Fingerprint::of(&a), Fingerprint::of(&b));
    }

    #[test]
    fn different_shape_different_fingerprint() {
        let a = RelExpr::scan("t");
        let b = RelExpr::scan("t").filter(Expr::Const(Const::Bool(true)));
        assert_ne!(Fingerprint::of(&a), Fingerprint::of(&b));
    }

    #[test]
    fn ngrams_extraction() {
        let fp = Fingerprint::of(&RelExpr::scan("t").filter(Expr::Const(Const::Bool(true))));
        let bigrams = fp.ngrams(2);
        assert_eq!(bigrams.len(), fp.len() - 1);
    }

    #[test]
    fn ngrams_too_large() {
        let fp = Fingerprint::of(&RelExpr::scan("t"));
        assert!(fp.ngrams(5).is_empty());
    }

    #[test]
    fn display() {
        let fp = Fingerprint::of(&RelExpr::scan("t"));
        assert_eq!(fp.to_string(), "SCAN");
    }
}

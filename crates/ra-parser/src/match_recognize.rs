//! Parser for SQL:2016 `MATCH_RECOGNIZE` clause.
//!
//! Converts a SQL query containing `MATCH_RECOGNIZE` into the
//! `RowPattern` relational algebra operator. Uses `sqlparser` for
//! initial SQL parsing, then extracts RPR-specific clauses.

use ra_core::algebra::{RelExpr, SortKey};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use ra_core::row_pattern::{
    MatchMode, PatternDefine, PatternExpr, PatternMeasure, Quantifier, SkipMode,
};
use thiserror::Error;

/// Errors from `MATCH_RECOGNIZE` parsing.
#[derive(Debug, Error)]
pub enum MatchRecognizeError {
    /// The pattern string is syntactically invalid.
    #[error("invalid pattern: {0}")]
    InvalidPattern(String),

    /// A DEFINE clause references an undefined variable.
    #[error("undefined pattern variable: {0}")]
    UndefinedVariable(String),

    /// A required clause is missing.
    #[error("missing required clause: {0}")]
    MissingClause(String),

    /// General parse error.
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Parse a PATTERN clause string into a `PatternExpr`.
///
/// Supports: variable names, sequences (space-separated),
/// alternation (`|`), quantifiers (`+`, `*`, `?`, `{n}`, `{n,m}`),
/// and grouping with parentheses.
///
/// # Errors
///
/// Returns error if the pattern string has unmatched parens,
/// empty groups, or invalid quantifiers.
pub fn parse_pattern(input: &str) -> Result<PatternExpr, MatchRecognizeError> {
    let tokens = tokenize_pattern(input)?;
    let (expr, rest) = parse_alternation(&tokens)?;
    if !rest.is_empty() {
        return Err(MatchRecognizeError::InvalidPattern(format!(
            "unexpected tokens after pattern: {rest:?}"
        )));
    }
    Ok(expr)
}

/// Build a `RowPattern` `RelExpr` from pre-parsed components.
///
/// # Errors
///
/// Returns error if required fields are missing or defines
/// reference variables not present in the pattern.
#[expect(
    clippy::too_many_arguments,
    reason = "Row pattern building requires many configuration parameters from SQL MATCH_RECOGNIZE clause"
)]
pub fn build_row_pattern(
    input: RelExpr,
    partition_by: Vec<Expr>,
    order_by: Vec<SortKey>,
    pattern: PatternExpr,
    defines: Vec<PatternDefine>,
    measures: Vec<PatternMeasure>,
    mode: Option<MatchMode>,
    skip_mode: Option<SkipMode>,
) -> Result<RelExpr, MatchRecognizeError> {
    if order_by.is_empty() {
        return Err(MatchRecognizeError::MissingClause(
            "ORDER BY is required for MATCH_RECOGNIZE".to_owned(),
        ));
    }

    let pattern_vars = pattern.variables();
    for define in &defines {
        if !pattern_vars.contains(&define.variable.as_str()) {
            return Err(MatchRecognizeError::UndefinedVariable(
                define.variable.clone(),
            ));
        }
    }

    Ok(RelExpr::RowPattern {
        input: Box::new(input),
        partition_by,
        order_by,
        pattern,
        defines,
        measures,
        mode: mode.unwrap_or(MatchMode::OneRowPerMatch),
        skip_mode: skip_mode.unwrap_or(SkipMode::PastLastRow),
    })
}

/// Parse a DEFINE condition expression from a simple string.
///
/// Supports: `column < PREV(column)`, `column > PREV(column)`,
/// and `column < PREV(column) AND expr`.
///
/// # Errors
///
/// Returns error if the expression syntax is not recognized.
pub fn parse_define_condition(input: &str) -> Result<Expr, MatchRecognizeError> {
    let input = input.trim();

    // Handle AND-joined conditions
    if let Some(and_pos) = find_top_level_and(input) {
        let left = parse_define_condition(&input[..and_pos])?;
        let right = parse_define_condition(&input[and_pos + 3..])?;
        return Ok(Expr::BinOp {
            op: BinOp::And,
            left: Box::new(left),
            right: Box::new(right),
        });
    }

    // Handle OR-joined conditions
    if let Some(or_pos) = find_top_level_or(input) {
        let left = parse_define_condition(&input[..or_pos])?;
        let right = parse_define_condition(&input[or_pos + 2..])?;
        return Ok(Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(left),
            right: Box::new(right),
        });
    }

    // Try comparison: expr op expr
    for (op_str, op) in &[
        ("<=", BinOp::Le),
        (">=", BinOp::Ge),
        ("!=", BinOp::Ne),
        ("<", BinOp::Lt),
        (">", BinOp::Gt),
        ("=", BinOp::Eq),
    ] {
        if let Some(pos) = find_comparison_op(input, op_str) {
            let left = parse_simple_expr(input[..pos].trim())?;
            let right = parse_simple_expr(input[pos + op_str.len()..].trim())?;
            return Ok(Expr::BinOp {
                op: *op,
                left: Box::new(left),
                right: Box::new(right),
            });
        }
    }

    // Fallback: treat as simple expression
    parse_simple_expr(input)
}

/// Parse MEASURES expression from string.
///
/// # Errors
///
/// Returns error if expression syntax is not recognized.
pub fn parse_measure_expr(input: &str) -> Result<Expr, MatchRecognizeError> {
    parse_simple_expr(input.trim())
}

// -- Internal tokenizer and parser for PATTERN clause --

#[derive(Debug, Clone, PartialEq)]
enum PatternToken {
    Ident(String),
    Plus,
    Star,
    Question,
    Pipe,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Number(usize),
}

fn tokenize_pattern(input: &str) -> Result<Vec<PatternToken>, MatchRecognizeError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '+' => {
                tokens.push(PatternToken::Plus);
                chars.next();
            }
            '*' => {
                tokens.push(PatternToken::Star);
                chars.next();
            }
            '?' => {
                tokens.push(PatternToken::Question);
                chars.next();
            }
            '|' => {
                tokens.push(PatternToken::Pipe);
                chars.next();
            }
            '(' => {
                tokens.push(PatternToken::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(PatternToken::RParen);
                chars.next();
            }
            '{' => {
                tokens.push(PatternToken::LBrace);
                chars.next();
            }
            '}' => {
                tokens.push(PatternToken::RBrace);
                chars.next();
            }
            ',' => {
                tokens.push(PatternToken::Comma);
                chars.next();
            }
            '0'..='9' => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        num.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let n: usize = num.parse().map_err(|_| {
                    MatchRecognizeError::InvalidPattern(format!("invalid number: {num}"))
                })?;
                tokens.push(PatternToken::Number(n));
            }
            _ if ch.is_alphanumeric() || ch == '_' => {
                let mut ident = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        ident.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(PatternToken::Ident(ident));
            }
            other => {
                return Err(MatchRecognizeError::InvalidPattern(format!(
                    "unexpected character: {other}"
                )));
            }
        }
    }

    Ok(tokens)
}

fn parse_alternation(
    tokens: &[PatternToken],
) -> Result<(PatternExpr, &[PatternToken]), MatchRecognizeError> {
    let (first, mut rest) = parse_sequence(tokens)?;
    let mut branches = vec![first];

    while rest.first() == Some(&PatternToken::Pipe) {
        let (next, new_rest) = parse_sequence(&rest[1..])?;
        branches.push(next);
        rest = new_rest;
    }

    if branches.len() == 1 {
        Ok((branches.remove(0), rest))
    } else {
        Ok((PatternExpr::Alternation(branches), rest))
    }
}

fn parse_sequence(
    tokens: &[PatternToken],
) -> Result<(PatternExpr, &[PatternToken]), MatchRecognizeError> {
    let mut parts = Vec::new();
    let mut rest = tokens;

    while !rest.is_empty() {
        // Stop at pipe or RParen (those belong to outer contexts)
        if rest.first() == Some(&PatternToken::Pipe) || rest.first() == Some(&PatternToken::RParen)
        {
            break;
        }

        let (atom, new_rest) = parse_quantified(rest)?;
        parts.push(atom);
        rest = new_rest;
    }

    if parts.is_empty() {
        return Err(MatchRecognizeError::InvalidPattern(
            "empty pattern sequence".to_owned(),
        ));
    }

    if parts.len() == 1 {
        Ok((parts.remove(0), rest))
    } else {
        Ok((PatternExpr::Sequence(parts), rest))
    }
}

fn parse_quantified(
    tokens: &[PatternToken],
) -> Result<(PatternExpr, &[PatternToken]), MatchRecognizeError> {
    let (base, rest) = parse_atom(tokens)?;
    parse_optional_quantifier(base, rest)
}

fn parse_optional_quantifier(
    base: PatternExpr,
    tokens: &[PatternToken],
) -> Result<(PatternExpr, &[PatternToken]), MatchRecognizeError> {
    if tokens.is_empty() {
        return Ok((base, tokens));
    }

    match tokens.first() {
        Some(PatternToken::Plus) => Ok((
            PatternExpr::Quantified(Box::new(base), Quantifier::OneOrMore),
            &tokens[1..],
        )),
        Some(PatternToken::Star) => Ok((
            PatternExpr::Quantified(Box::new(base), Quantifier::ZeroOrMore),
            &tokens[1..],
        )),
        Some(PatternToken::Question) => Ok((
            PatternExpr::Quantified(Box::new(base), Quantifier::ZeroOrOne),
            &tokens[1..],
        )),
        Some(PatternToken::LBrace) => {
            let (quant, rest) = parse_brace_quantifier(&tokens[1..])?;
            Ok((PatternExpr::Quantified(Box::new(base), quant), rest))
        }
        _ => Ok((base, tokens)),
    }
}

fn parse_brace_quantifier(
    tokens: &[PatternToken],
) -> Result<(Quantifier, &[PatternToken]), MatchRecognizeError> {
    // Expect: Number [, Number?] }
    let Some(PatternToken::Number(min)) = tokens.first() else {
        return Err(MatchRecognizeError::InvalidPattern(
            "expected number after '{'".to_owned(),
        ));
    };

    let rest = &tokens[1..];

    if rest.first() == Some(&PatternToken::RBrace) {
        return Ok((Quantifier::Exactly(*min), &rest[1..]));
    }

    if rest.first() != Some(&PatternToken::Comma) {
        return Err(MatchRecognizeError::InvalidPattern(
            "expected ',' or '}' in quantifier".to_owned(),
        ));
    }

    let rest = &rest[1..]; // skip comma

    if rest.first() == Some(&PatternToken::RBrace) {
        return Ok((Quantifier::Range(*min, None), &rest[1..]));
    }

    let Some(PatternToken::Number(max)) = rest.first() else {
        return Err(MatchRecognizeError::InvalidPattern(
            "expected number or '}' after ','".to_owned(),
        ));
    };

    let rest = &rest[1..];
    if rest.first() != Some(&PatternToken::RBrace) {
        return Err(MatchRecognizeError::InvalidPattern(
            "expected '}' to close quantifier".to_owned(),
        ));
    }

    Ok((Quantifier::Range(*min, Some(*max)), &rest[1..]))
}

fn parse_atom(
    tokens: &[PatternToken],
) -> Result<(PatternExpr, &[PatternToken]), MatchRecognizeError> {
    if tokens.is_empty() {
        return Err(MatchRecognizeError::InvalidPattern(
            "unexpected end of pattern".to_owned(),
        ));
    }

    match &tokens[0] {
        PatternToken::Ident(name) => Ok((PatternExpr::Var(name.clone()), &tokens[1..])),
        PatternToken::LParen => {
            let (inner, rest) = parse_alternation(&tokens[1..])?;
            if rest.first() != Some(&PatternToken::RParen) {
                return Err(MatchRecognizeError::InvalidPattern(
                    "unmatched '('".to_owned(),
                ));
            }
            Ok((PatternExpr::Group(Box::new(inner)), &rest[1..]))
        }
        other => Err(MatchRecognizeError::InvalidPattern(format!(
            "unexpected token: {other:?}"
        ))),
    }
}

// -- Helpers for DEFINE expression parsing --

fn find_top_level_and(input: &str) -> Option<usize> {
    find_top_level_keyword(input, " AND ")
}

fn find_top_level_or(input: &str) -> Option<usize> {
    find_top_level_keyword(input, " OR ")
}

fn find_top_level_keyword(input: &str, keyword: &str) -> Option<usize> {
    let upper = input.to_uppercase();
    let mut depth = 0i32;
    let bytes = upper.as_bytes();

    for i in 0..bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && upper[i..].starts_with(keyword) {
            return Some(i);
        }
    }
    None
}

fn find_comparison_op(input: &str, op: &str) -> Option<usize> {
    let mut depth = 0i32;

    for i in 0..input.len() {
        match input.as_bytes()[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && input[i..].starts_with(op) {
            // Avoid matching <= when looking for <, etc.
            if op == "<" && input[i..].starts_with("<=") {
                continue;
            }
            if op == ">" && input[i..].starts_with(">=") {
                continue;
            }
            if op == "!" && input[i..].starts_with("!=") {
                continue;
            }
            return Some(i);
        }
    }
    None
}

fn parse_simple_expr(input: &str) -> Result<Expr, MatchRecognizeError> {
    let input = input.trim();

    if input.is_empty() {
        return Err(MatchRecognizeError::ParseError(
            "empty expression".to_owned(),
        ));
    }

    // PREV(column) or PREV(column, n)
    let upper = input.to_uppercase();
    if upper.starts_with("PREV(") && input.ends_with(')') {
        let inner = &input[5..input.len() - 1];
        return parse_prev_next(inner, true);
    }

    // NEXT(column) or NEXT(column, n)
    if upper.starts_with("NEXT(") && input.ends_with(')') {
        let inner = &input[5..input.len() - 1];
        return parse_prev_next(inner, false);
    }

    // FIRST(var.column)
    if upper.starts_with("FIRST(") && input.ends_with(')') {
        let inner = &input[6..input.len() - 1];
        return parse_first_last(inner, true);
    }

    // LAST(var.column)
    if upper.starts_with("LAST(") && input.ends_with(')') {
        let inner = &input[5..input.len() - 1];
        return parse_first_last(inner, false);
    }

    // CLASSIFIER()
    if upper == "CLASSIFIER()" {
        return Ok(Expr::PatternClassifier);
    }

    // MATCH_NUMBER()
    if upper == "MATCH_NUMBER()" {
        return Ok(Expr::PatternMatchNumber);
    }

    // Numeric literal
    if let Ok(n) = input.parse::<i64>() {
        return Ok(Expr::Const(Const::Int(n)));
    }
    if let Ok(f) = input.parse::<f64>() {
        return Ok(Expr::Const(Const::Float(f)));
    }

    // String literal
    if input.starts_with('\'') && input.ends_with('\'') {
        let s = &input[1..input.len() - 1];
        return Ok(Expr::Const(Const::String(s.to_owned())));
    }

    // Qualified column (var.column)
    if let Some(dot_pos) = input.find('.') {
        let table = &input[..dot_pos];
        let column = &input[dot_pos + 1..];
        return Ok(Expr::Column(ColumnRef::qualified(table, column)));
    }

    // Unqualified column
    Ok(Expr::Column(ColumnRef::new(input)))
}

fn parse_prev_next(inner: &str, is_prev: bool) -> Result<Expr, MatchRecognizeError> {
    let parts: Vec<&str> = inner.splitn(2, ',').collect();
    let col_str = parts[0].trim();
    let offset: usize = if parts.len() > 1 {
        parts[1].trim().parse().map_err(|_| {
            MatchRecognizeError::ParseError(format!("invalid offset: {}", parts[1].trim()))
        })?
    } else {
        1
    };

    let col_expr = parse_simple_expr(col_str)?;

    if is_prev {
        Ok(Expr::PatternPrev(Box::new(col_expr), offset))
    } else {
        Ok(Expr::PatternNext(Box::new(col_expr), offset))
    }
}

fn parse_first_last(inner: &str, is_first: bool) -> Result<Expr, MatchRecognizeError> {
    let inner = inner.trim();
    // Expect var.column
    let Some(dot_pos) = inner.find('.') else {
        return Err(MatchRecognizeError::ParseError(format!(
            "FIRST/LAST requires var.column syntax, got: {inner}"
        )));
    };

    let var = &inner[..dot_pos];
    let column = &inner[dot_pos + 1..];
    let col_expr = Expr::Column(ColumnRef::new(column));

    if is_first {
        Ok(Expr::PatternFirst(Box::new(col_expr), var.to_owned()))
    } else {
        Ok(Expr::PatternLast(Box::new(col_expr), var.to_owned()))
    }
}

#[expect(clippy::expect_used, clippy::panic, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::{NullOrdering, SortDirection};

    // -- Pattern parsing tests --

    #[test]
    fn parse_single_var() {
        let p = parse_pattern("A").expect("should parse");
        assert_eq!(p, PatternExpr::Var("A".to_owned()));
    }

    #[test]
    fn parse_sequence() {
        let p = parse_pattern("A B C").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Sequence(vec![
                PatternExpr::Var("A".to_owned()),
                PatternExpr::Var("B".to_owned()),
                PatternExpr::Var("C".to_owned()),
            ])
        );
    }

    #[test]
    fn parse_alternation() {
        let p = parse_pattern("A | B").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Alternation(vec![
                PatternExpr::Var("A".to_owned()),
                PatternExpr::Var("B".to_owned()),
            ])
        );
    }

    #[test]
    fn parse_quantifier_plus() {
        let p = parse_pattern("A+").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::OneOrMore,
            )
        );
    }

    #[test]
    fn parse_quantifier_star() {
        let p = parse_pattern("A*").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::ZeroOrMore,
            )
        );
    }

    #[test]
    fn parse_quantifier_question() {
        let p = parse_pattern("A?").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::ZeroOrOne,
            )
        );
    }

    #[test]
    fn parse_quantifier_exactly() {
        let p = parse_pattern("A{3}").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::Exactly(3),
            )
        );
    }

    #[test]
    fn parse_quantifier_range() {
        let p = parse_pattern("A{2,5}").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::Range(2, Some(5)),
            )
        );
    }

    #[test]
    fn parse_quantifier_range_unbounded() {
        let p = parse_pattern("A{2,}").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Quantified(
                Box::new(PatternExpr::Var("A".to_owned())),
                Quantifier::Range(2, None),
            )
        );
    }

    #[test]
    fn parse_group() {
        let p = parse_pattern("(A B)+").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Quantified(
                Box::new(PatternExpr::Group(Box::new(PatternExpr::Sequence(vec![
                    PatternExpr::Var("A".to_owned()),
                    PatternExpr::Var("B".to_owned()),
                ])))),
                Quantifier::OneOrMore,
            )
        );
    }

    #[test]
    fn parse_complex_v_pattern() {
        let p = parse_pattern("A+ B{2,5} C+").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Sequence(vec![
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
            ])
        );
    }

    #[test]
    fn parse_alternation_with_sequence() {
        let p = parse_pattern("A B | C D").expect("should parse");
        assert_eq!(
            p,
            PatternExpr::Alternation(vec![
                PatternExpr::Sequence(vec![
                    PatternExpr::Var("A".to_owned()),
                    PatternExpr::Var("B".to_owned()),
                ]),
                PatternExpr::Sequence(vec![
                    PatternExpr::Var("C".to_owned()),
                    PatternExpr::Var("D".to_owned()),
                ]),
            ])
        );
    }

    #[test]
    fn parse_nested_groups() {
        let p = parse_pattern("(A (B | C))+").expect("should parse");
        if let PatternExpr::Quantified(inner, q) = &p {
            assert_eq!(*q, Quantifier::OneOrMore);
            if let PatternExpr::Group(group) = inner.as_ref() {
                assert!(matches!(group.as_ref(), PatternExpr::Sequence(_)));
            } else {
                panic!("expected Group");
            }
        } else {
            panic!("expected Quantified");
        }
    }

    #[test]
    fn parse_empty_pattern_error() {
        let result = parse_pattern("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_unmatched_paren_error() {
        let result = parse_pattern("(A B");
        assert!(result.is_err());
    }

    // -- DEFINE condition parsing tests --

    #[test]
    fn parse_simple_comparison() {
        let expr = parse_define_condition("price < PREV(price)").expect("should parse");
        if let Expr::BinOp { op, left, right } = &expr {
            assert_eq!(*op, BinOp::Lt);
            assert!(matches!(left.as_ref(), Expr::Column(_)));
            assert!(matches!(right.as_ref(), Expr::PatternPrev(_, 1)));
        } else {
            panic!("expected BinOp");
        }
    }

    #[test]
    fn parse_prev_with_offset() {
        let expr = parse_define_condition("price < PREV(price, 2)").expect("should parse");
        if let Expr::BinOp { right, .. } = &expr {
            assert!(matches!(right.as_ref(), Expr::PatternPrev(_, 2)));
        } else {
            panic!("expected BinOp");
        }
    }

    #[test]
    fn parse_and_condition() {
        let expr = parse_define_condition("price < PREV(price) AND volume > 1000000")
            .expect("should parse");
        assert!(matches!(expr, Expr::BinOp { op: BinOp::And, .. }));
    }

    #[test]
    fn parse_next_expr() {
        let expr = parse_define_condition("price > NEXT(price)").expect("should parse");
        if let Expr::BinOp { right, .. } = &expr {
            assert!(matches!(right.as_ref(), Expr::PatternNext(_, 1)));
        } else {
            panic!("expected BinOp");
        }
    }

    // -- MEASURES parsing tests --

    #[test]
    fn parse_first_measure() {
        let expr = parse_measure_expr("FIRST(A.price)").expect("should parse");
        if let Expr::PatternFirst(inner, var) = &expr {
            assert_eq!(var, "A");
            assert!(matches!(inner.as_ref(), Expr::Column(_)));
        } else {
            panic!("expected PatternFirst, got {expr:?}");
        }
    }

    #[test]
    fn parse_last_measure() {
        let expr = parse_measure_expr("LAST(B.price)").expect("should parse");
        if let Expr::PatternLast(inner, var) = &expr {
            assert_eq!(var, "B");
            assert!(matches!(inner.as_ref(), Expr::Column(_)));
        } else {
            panic!("expected PatternLast");
        }
    }

    #[test]
    fn parse_classifier_expr() {
        let expr = parse_measure_expr("CLASSIFIER()").expect("should parse");
        assert_eq!(expr, Expr::PatternClassifier);
    }

    #[test]
    fn parse_match_number_expr() {
        let expr = parse_measure_expr("MATCH_NUMBER()").expect("should parse");
        assert_eq!(expr, Expr::PatternMatchNumber);
    }

    // -- build_row_pattern tests --

    #[test]
    fn build_valid_row_pattern() {
        let input = RelExpr::scan("stock_prices");
        let order_by = vec![SortKey {
            expr: Expr::Column(ColumnRef::new("trade_date")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }];
        let pattern = parse_pattern("A+ B+ C+").expect("should parse");
        let defines = vec![
            PatternDefine {
                variable: "A".to_owned(),
                condition: Expr::Const(Const::Bool(true)),
            },
            PatternDefine {
                variable: "B".to_owned(),
                condition: Expr::Const(Const::Bool(true)),
            },
            PatternDefine {
                variable: "C".to_owned(),
                condition: Expr::Const(Const::Bool(true)),
            },
        ];

        let result = build_row_pattern(
            input,
            vec![],
            order_by,
            pattern,
            defines,
            vec![],
            None,
            None,
        );
        assert!(result.is_ok());

        let plan = result.expect("already checked");
        assert!(matches!(plan, RelExpr::RowPattern { .. }));
    }

    #[test]
    fn build_row_pattern_missing_order_by() {
        let input = RelExpr::scan("t");
        let result = build_row_pattern(
            input,
            vec![],
            vec![],
            PatternExpr::Var("A".to_owned()),
            vec![],
            vec![],
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn build_row_pattern_undefined_variable() {
        let input = RelExpr::scan("t");
        let order_by = vec![SortKey {
            expr: Expr::Column(ColumnRef::new("ts")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }];
        let defines = vec![PatternDefine {
            variable: "X".to_owned(),
            condition: Expr::Const(Const::Bool(true)),
        }];

        let result = build_row_pattern(
            input,
            vec![],
            order_by,
            PatternExpr::Var("A".to_owned()),
            defines,
            vec![],
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn build_row_pattern_with_all_options() {
        let input = RelExpr::scan("events");
        let partition_by = vec![Expr::Column(ColumnRef::new("category"))];
        let order_by = vec![SortKey {
            expr: Expr::Column(ColumnRef::new("event_time")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }];
        let pattern = parse_pattern("A+ B?").expect("should parse");
        let defines = vec![
            PatternDefine {
                variable: "A".to_owned(),
                condition: Expr::Const(Const::Bool(true)),
            },
            PatternDefine {
                variable: "B".to_owned(),
                condition: Expr::Const(Const::Bool(true)),
            },
        ];
        let measures = vec![PatternMeasure {
            expr: Expr::PatternFirst(
                Box::new(Expr::Column(ColumnRef::new("value"))),
                "A".to_owned(),
            ),
            alias: "start_value".to_owned(),
        }];

        let result = build_row_pattern(
            input,
            partition_by,
            order_by,
            pattern,
            defines,
            measures,
            Some(MatchMode::AllRowsPerMatch),
            Some(SkipMode::ToNextRow),
        );
        assert!(result.is_ok());

        if let RelExpr::RowPattern {
            mode, skip_mode, ..
        } = result.expect("already checked")
        {
            assert_eq!(mode, MatchMode::AllRowsPerMatch);
            assert_eq!(skip_mode, SkipMode::ToNextRow);
        } else {
            panic!("expected RowPattern");
        }
    }

    #[test]
    fn row_pattern_children() {
        let input = RelExpr::scan("t");
        let order_by = vec![SortKey {
            expr: Expr::Column(ColumnRef::new("ts")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }];
        let plan = build_row_pattern(
            input,
            vec![],
            order_by,
            PatternExpr::Var("A".to_owned()),
            vec![],
            vec![],
            None,
            None,
        )
        .expect("should build");

        assert_eq!(plan.children().len(), 1);
    }

    #[test]
    fn row_pattern_references_cte() {
        let input = RelExpr::scan("my_cte");
        let order_by = vec![SortKey {
            expr: Expr::Column(ColumnRef::new("ts")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::Last,
        }];
        let plan = build_row_pattern(
            input,
            vec![],
            order_by,
            PatternExpr::Var("A".to_owned()),
            vec![],
            vec![],
            None,
            None,
        )
        .expect("should build");

        assert!(plan.references_cte("my_cte"));
        assert!(!plan.references_cte("other"));
    }
}

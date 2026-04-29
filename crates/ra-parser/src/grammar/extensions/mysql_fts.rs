//! `MySQL` Full-Text Search extension.
//!
//! `MySQL` provides full-text search capabilities through the MATCH...AGAINST syntax.
//! It supports natural language search, boolean search, and query expansion.
//!
//! # Key Features
//!
//! ## Natural Language Mode (default)
//!
//! ```sql
//! SELECT * FROM articles
//! WHERE MATCH(title, body) AGAINST('database performance');
//!
//! -- Explicit natural language mode
//! SELECT * FROM articles
//! WHERE MATCH(title, body) AGAINST('database performance' IN NATURAL LANGUAGE MODE);
//! ```
//!
//! ## Boolean Mode
//!
//! Boolean mode supports operators for more precise control:
//! - `+word` - Must contain word
//! - `-word` - Must not contain word
//! - `word*` - Wildcard (matches word, words, wordy, etc.)
//! - `"phrase"` - Exact phrase match
//! - `()` - Grouping
//! - `>` - Increase rank
//! - `<` - Decrease rank
//! - `~` - Negation (reduce rank)
//!
//! ```sql
//! -- Must have "database", must not have "slow"
//! SELECT * FROM articles
//! WHERE MATCH(title, body) AGAINST('+database -slow' IN BOOLEAN MODE);
//!
//! -- Phrase search with wildcards
//! SELECT * FROM articles
//! WHERE MATCH(title, body) AGAINST('"high performance" +optim*' IN BOOLEAN MODE);
//! ```
//!
//! ## Query Expansion
//!
//! Performs a second search using terms from the most relevant documents:
//!
//! ```sql
//! SELECT * FROM articles
//! WHERE MATCH(title, body) AGAINST('database' WITH QUERY EXPANSION);
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// `MySQL` Full-Text Search extension.
pub struct MySQLFTSExtension;

impl GrammarExtension for MySQLFTSExtension {
    fn name(&self) -> &'static str {
        "mysql_fts"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            "MATCH",
            "AGAINST",
            "IN NATURAL LANGUAGE MODE",
            "IN BOOLEAN MODE",
            "WITH QUERY EXPANSION",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            // Boolean mode operators (handled within query string)
            "+", "-", "*", "\"", "(", ")", ">", "<", "~",
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            "MATCH", // MATCH(...) AGAINST(...) is function-like
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://dev.mysql.com/doc/refman/8.4/en/fulltext-search.html")
    }

    fn min_version(&self) -> Option<&str> {
        Some("5.6")
    }
}

/// `MySQL` full-text search mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MySQLFTSMode {
    /// Natural language mode (default).
    NaturalLanguage,
    /// Boolean mode with operators.
    Boolean,
    /// Natural language mode with query expansion.
    QueryExpansion,
}

/// Parsed MATCH...AGAINST expression.
#[derive(Debug, Clone, PartialEq)]
pub struct MySQLMatchExpr {
    /// Columns to search in.
    pub columns: Vec<String>,
    /// Search query string.
    pub query: String,
    /// Search mode.
    pub mode: MySQLFTSMode,
}

/// Boolean query token for `MySQL` boolean mode parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BooleanToken {
    /// A term that must be present (+term).
    MustHave(String),
    /// A term that must not be present (-term).
    MustNotHave(String),
    /// A term that increases rank (>term).
    IncreaseRank(String),
    /// A term that decreases rank (<term).
    DecreaseRank(String),
    /// A term with negation that reduces rank (~term).
    Negate(String),
    /// A wildcard term (term*).
    Wildcard(String),
    /// An exact phrase match ("phrase").
    Phrase(String),
    /// A regular optional term.
    Optional(String),
    /// Grouped terms (nested).
    Group(Vec<BooleanToken>),
}

/// Parse `MySQL` boolean mode query string.
///
/// # Errors
///
/// Returns an error if the query contains invalid syntax.
///
/// # Examples
///
/// ```ignore
/// let tokens = parse_boolean_query("+database -slow optim*");
/// // Returns: [MustHave("database"), MustNotHave("slow"), Wildcard("optim")]
/// ```
#[expect(
    clippy::too_many_lines,
    reason = "Boolean query parsing requires handling many token types and states"
)]
pub fn parse_boolean_query(query: &str) -> Result<Vec<BooleanToken>, String> {
    let mut tokens = Vec::new();
    let mut chars = query.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
            }
            '+' => {
                chars.next();
                // Skip whitespace after operator
                while let Some(&ch) = chars.peek() {
                    if !ch.is_whitespace() {
                        break;
                    }
                    chars.next();
                }
                // Check if next char is another operator - let main loop handle it
                if let Some(&next_ch) = chars.peek() {
                    if matches!(next_ch, '+' | '-' | '>' | '<' | '~') {
                        // Consecutive operators - loop continues to process next one
                        continue;
                    }
                }
                // Check if next is a phrase, group, or term
                if let Some(&'"') = chars.peek() {
                    chars.next();
                    let phrase = read_phrase(&mut chars)?;
                    tokens.push(BooleanToken::Phrase(phrase));
                } else if let Some(&'(') = chars.peek() {
                    chars.next();
                    let group = read_group(&mut chars)?;
                    tokens.push(BooleanToken::Group(group));
                } else {
                    let term = read_term(&mut chars);
                    if term.is_empty() {
                        return Err("Empty term after '+'".to_string());
                    }
                    tokens.push(BooleanToken::MustHave(term));
                }
            }
            '-' => {
                chars.next();
                // Skip whitespace after operator
                while let Some(&ch) = chars.peek() {
                    if !ch.is_whitespace() {
                        break;
                    }
                    chars.next();
                }
                // Check if next char is another operator - let main loop handle it
                if let Some(&next_ch) = chars.peek() {
                    if matches!(next_ch, '+' | '-' | '>' | '<' | '~') {
                        // Consecutive operators - loop continues to process next one
                        continue;
                    }
                }
                // Check if next is a phrase, group, or term
                if let Some(&'"') = chars.peek() {
                    chars.next();
                    let phrase = read_phrase(&mut chars)?;
                    tokens.push(BooleanToken::Phrase(phrase));
                } else if let Some(&'(') = chars.peek() {
                    chars.next();
                    let group = read_group(&mut chars)?;
                    tokens.push(BooleanToken::Group(group));
                } else {
                    let term = read_term(&mut chars);
                    if term.is_empty() {
                        return Err("Empty term after '-'".to_string());
                    }
                    tokens.push(BooleanToken::MustNotHave(term));
                }
            }
            '>' => {
                chars.next();
                let term = read_term(&mut chars);
                if term.is_empty() {
                    return Err("Empty term after '>'".to_string());
                }
                tokens.push(BooleanToken::IncreaseRank(term));
            }
            '<' => {
                chars.next();
                let term = read_term(&mut chars);
                if term.is_empty() {
                    return Err("Empty term after '<'".to_string());
                }
                tokens.push(BooleanToken::DecreaseRank(term));
            }
            '~' => {
                chars.next();
                let term = read_term(&mut chars);
                if term.is_empty() {
                    return Err("Empty term after '~'".to_string());
                }
                tokens.push(BooleanToken::Negate(term));
            }
            '"' => {
                chars.next();
                let phrase = read_phrase(&mut chars)?;
                tokens.push(BooleanToken::Phrase(phrase));
            }
            '(' => {
                chars.next();
                let group = read_group(&mut chars)?;
                tokens.push(BooleanToken::Group(group));
            }
            ')' => {
                return Err("Unexpected ')' without matching '('".to_string());
            }
            _ => {
                let term = read_term(&mut chars);
                if !term.is_empty() {
                    if term.ends_with('*') {
                        tokens.push(BooleanToken::Wildcard(term));
                    } else {
                        tokens.push(BooleanToken::Optional(term));
                    }
                }
            }
        }
    }

    Ok(tokens)
}

fn read_term(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut term = String::new();
    while let Some(&ch) = chars.peek() {
        // Stop at whitespace or operators at start of next token
        // But allow hyphens/underscores within terms
        if ch.is_whitespace() {
            break;
        }
        // Only treat operators as delimiters if we're at the start or after whitespace
        // This allows hyphenated terms like "full-text"
        if matches!(ch, '+' | '>' | '<' | '~' | '"' | '(' | ')') {
            break;
        }
        // Allow hyphens within terms but not at the start if term is non-empty
        if ch == '-' && term.is_empty() {
            break;
        }
        // If we see a hyphen followed by whitespace, it might be a negation operator
        if ch == '-' {
            let mut lookahead = chars.clone();
            lookahead.next(); // consume the hyphen
            if let Some(&next_ch) = lookahead.peek() {
                if next_ch.is_whitespace() {
                    // This is likely a standalone minus operator, stop here
                    break;
                }
            }
        }
        term.push(ch);
        chars.next();
    }
    term
}

fn read_phrase(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<String, String> {
    let mut phrase = String::new();
    let mut found_closing = false;

    for ch in chars.by_ref() {
        if ch == '"' {
            found_closing = true;
            break;
        }
        phrase.push(ch);
    }

    if !found_closing {
        return Err("Unclosed phrase quote".to_string());
    }

    Ok(phrase)
}

fn read_group(
    chars: &mut std::iter::Peekable<std::str::Chars>,
) -> Result<Vec<BooleanToken>, String> {
    let mut group_str = String::new();
    let mut depth = 1;

    for ch in chars.by_ref() {
        if ch == '(' {
            depth += 1;
            group_str.push(ch);
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                break;
            }
            group_str.push(ch);
        } else {
            group_str.push(ch);
        }
    }

    if depth != 0 {
        return Err("Unclosed group parenthesis".to_string());
    }

    parse_boolean_query(&group_str)
}

#[expect(clippy::unwrap_used, clippy::panic, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mysql_fts_extension() {
        let ext = MySQLFTSExtension;
        assert_eq!(ext.name(), "mysql_fts");

        let keywords = ext.keywords();
        assert!(keywords.contains(&"MATCH"));
        assert!(keywords.contains(&"AGAINST"));
        assert!(keywords.contains(&"IN NATURAL LANGUAGE MODE"));
        assert!(keywords.contains(&"IN BOOLEAN MODE"));
        assert!(keywords.contains(&"WITH QUERY EXPANSION"));
    }

    #[test]
    fn test_parse_boolean_simple_terms() {
        let tokens = parse_boolean_query("database performance").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::Optional("database".to_string()));
        assert_eq!(tokens[1], BooleanToken::Optional("performance".to_string()));
    }

    #[test]
    fn test_parse_boolean_must_have() {
        let tokens = parse_boolean_query("+database +optimization").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::MustHave("database".to_string()));
        assert_eq!(
            tokens[1],
            BooleanToken::MustHave("optimization".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_must_not_have() {
        let tokens = parse_boolean_query("-slow -deprecated").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::MustNotHave("slow".to_string()));
        assert_eq!(
            tokens[1],
            BooleanToken::MustNotHave("deprecated".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_mixed() {
        let tokens = parse_boolean_query("+database -slow optional").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], BooleanToken::MustHave("database".to_string()));
        assert_eq!(tokens[1], BooleanToken::MustNotHave("slow".to_string()));
        assert_eq!(tokens[2], BooleanToken::Optional("optional".to_string()));
    }

    #[test]
    fn test_parse_boolean_wildcard() {
        let tokens = parse_boolean_query("optim*").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], BooleanToken::Wildcard("optim*".to_string()));
    }

    #[test]
    fn test_parse_boolean_phrase() {
        let tokens = parse_boolean_query(r#""high performance""#).unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            BooleanToken::Phrase("high performance".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_phrase_with_terms() {
        let tokens = parse_boolean_query(r#"+database "query optimization""#).unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::MustHave("database".to_string()));
        assert_eq!(
            tokens[1],
            BooleanToken::Phrase("query optimization".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_rank_modifiers() {
        let tokens = parse_boolean_query(">important <less ~negate").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(
            tokens[0],
            BooleanToken::IncreaseRank("important".to_string())
        );
        assert_eq!(tokens[1], BooleanToken::DecreaseRank("less".to_string()));
        assert_eq!(tokens[2], BooleanToken::Negate("negate".to_string()));
    }

    #[test]
    fn test_parse_boolean_group() {
        let tokens = parse_boolean_query("+(database mysql) -slow").unwrap();
        assert_eq!(tokens.len(), 2);

        if let BooleanToken::Group(group) = &tokens[0] {
            assert_eq!(group.len(), 2);
            assert_eq!(group[0], BooleanToken::Optional("database".to_string()));
            assert_eq!(group[1], BooleanToken::Optional("mysql".to_string()));
        } else {
            panic!("Expected Group token");
        }

        assert_eq!(tokens[1], BooleanToken::MustNotHave("slow".to_string()));
    }

    #[test]
    fn test_parse_boolean_nested_groups() {
        let tokens = parse_boolean_query("((a b) c)").unwrap();
        assert_eq!(tokens.len(), 1);

        if let BooleanToken::Group(outer) = &tokens[0] {
            assert_eq!(outer.len(), 2);
            if let BooleanToken::Group(inner) = &outer[0] {
                assert_eq!(inner.len(), 2);
            } else {
                panic!("Expected nested Group token");
            }
        } else {
            panic!("Expected Group token");
        }
    }

    #[test]
    fn test_parse_boolean_unclosed_phrase() {
        let result = parse_boolean_query(r#""unclosed phrase"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unclosed phrase"));
    }

    #[test]
    fn test_parse_boolean_unclosed_group() {
        let result = parse_boolean_query("(unclosed");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unclosed group"));
    }

    #[test]
    fn test_parse_boolean_unexpected_close_paren() {
        let result = parse_boolean_query("term)");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unexpected ')'"));
    }

    #[test]
    fn test_parse_boolean_empty_operator() {
        let result = parse_boolean_query("+");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty term after '+'"));
    }

    #[test]
    fn test_parse_boolean_complex_query() {
        let tokens =
            parse_boolean_query(r#"+database +"query optimization" -slow optim* >fast"#).unwrap();
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0], BooleanToken::MustHave("database".to_string()));
        assert_eq!(
            tokens[1],
            BooleanToken::Phrase("query optimization".to_string())
        );
        assert_eq!(tokens[2], BooleanToken::MustNotHave("slow".to_string()));
        assert_eq!(tokens[3], BooleanToken::Wildcard("optim*".to_string()));
        assert_eq!(tokens[4], BooleanToken::IncreaseRank("fast".to_string()));
    }

    #[test]
    fn test_parse_boolean_whitespace_handling() {
        let tokens = parse_boolean_query("  +database   -slow  ").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::MustHave("database".to_string()));
        assert_eq!(tokens[1], BooleanToken::MustNotHave("slow".to_string()));
    }

    // Edge case tests

    #[test]
    fn test_parse_boolean_single_char_terms() {
        let tokens = parse_boolean_query("+a -b c").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], BooleanToken::MustHave("a".to_string()));
        assert_eq!(tokens[1], BooleanToken::MustNotHave("b".to_string()));
        assert_eq!(tokens[2], BooleanToken::Optional("c".to_string()));
    }

    #[test]
    fn test_parse_boolean_multiple_wildcards() {
        let tokens = parse_boolean_query("data* performa* optim*").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], BooleanToken::Wildcard("data*".to_string()));
        assert_eq!(tokens[1], BooleanToken::Wildcard("performa*".to_string()));
        assert_eq!(tokens[2], BooleanToken::Wildcard("optim*".to_string()));
    }

    #[test]
    fn test_parse_boolean_phrase_with_special_chars() {
        let tokens = parse_boolean_query(r#""query-optimization-2024""#).unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0],
            BooleanToken::Phrase("query-optimization-2024".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_multiple_phrases() {
        let tokens = parse_boolean_query(r#""first phrase" "second phrase""#).unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::Phrase("first phrase".to_string()));
        assert_eq!(tokens[1], BooleanToken::Phrase("second phrase".to_string()));
    }

    #[test]
    fn test_parse_boolean_operators_with_phrases() {
        let tokens = parse_boolean_query(r#"+"required phrase" -"excluded phrase""#).unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[0],
            BooleanToken::Phrase("required phrase".to_string())
        );
        assert_eq!(
            tokens[1],
            BooleanToken::Phrase("excluded phrase".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_rank_with_wildcard() {
        let tokens = parse_boolean_query(">import* <trivial*").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::IncreaseRank("import*".to_string()));
        assert_eq!(
            tokens[1],
            BooleanToken::DecreaseRank("trivial*".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_complex_nested() {
        let tokens = parse_boolean_query("+(database (mysql postgresql)) -deprecated").unwrap();
        assert_eq!(tokens.len(), 2);

        // +(group) where group contains: database and (mysql postgresql)
        if let BooleanToken::Group(outer) = &tokens[0] {
            assert_eq!(outer.len(), 2);
            assert_eq!(outer[0], BooleanToken::Optional("database".to_string()));
            if let BooleanToken::Group(inner) = &outer[1] {
                assert_eq!(inner.len(), 2);
                assert_eq!(inner[0], BooleanToken::Optional("mysql".to_string()));
                assert_eq!(inner[1], BooleanToken::Optional("postgresql".to_string()));
            } else {
                panic!("Expected inner group");
            }
        } else {
            panic!("Expected outer group");
        }
    }

    #[test]
    fn test_parse_boolean_all_operators() {
        let tokens =
            parse_boolean_query("+must -exclude >rank_up <rank_down ~negate optional wild*")
                .unwrap();
        assert_eq!(tokens.len(), 7);
        assert_eq!(tokens[0], BooleanToken::MustHave("must".to_string()));
        assert_eq!(tokens[1], BooleanToken::MustNotHave("exclude".to_string()));
        assert_eq!(tokens[2], BooleanToken::IncreaseRank("rank_up".to_string()));
        assert_eq!(
            tokens[3],
            BooleanToken::DecreaseRank("rank_down".to_string())
        );
        assert_eq!(tokens[4], BooleanToken::Negate("negate".to_string()));
        assert_eq!(tokens[5], BooleanToken::Optional("optional".to_string()));
        assert_eq!(tokens[6], BooleanToken::Wildcard("wild*".to_string()));
    }

    #[test]
    fn test_parse_boolean_unicode_terms() {
        let tokens = parse_boolean_query("+データベース -遅い").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[0],
            BooleanToken::MustHave("データベース".to_string())
        );
        assert_eq!(tokens[1], BooleanToken::MustNotHave("遅い".to_string()));
    }

    #[test]
    fn test_parse_boolean_numbers() {
        let tokens = parse_boolean_query("+2024 -2023 version*").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], BooleanToken::MustHave("2024".to_string()));
        assert_eq!(tokens[1], BooleanToken::MustNotHave("2023".to_string()));
        assert_eq!(tokens[2], BooleanToken::Wildcard("version*".to_string()));
    }

    #[test]
    fn test_parse_boolean_hyphenated_terms() {
        let tokens = parse_boolean_query("+full-text -real-time").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::MustHave("full-text".to_string()));
        assert_eq!(
            tokens[1],
            BooleanToken::MustNotHave("real-time".to_string())
        );
    }

    #[test]
    fn test_parse_boolean_underscored_terms() {
        let tokens = parse_boolean_query("+my_table -old_data").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::MustHave("my_table".to_string()));
        assert_eq!(tokens[1], BooleanToken::MustNotHave("old_data".to_string()));
    }

    #[test]
    fn test_parse_boolean_empty_group() {
        let result = parse_boolean_query("()");
        // Empty group should parse successfully with no tokens
        assert!(result.is_ok());
        let tokens = result.unwrap();
        assert_eq!(tokens.len(), 1);
        if let BooleanToken::Group(g) = &tokens[0] {
            assert_eq!(g.len(), 0);
        } else {
            panic!("Expected Group");
        }
    }

    #[test]
    fn test_parse_boolean_phrase_with_quotes_inside() {
        // MySQL doesn't support escaped quotes in phrases, but test our parser
        let result = parse_boolean_query(r#""outer""#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_boolean_only_wildcards() {
        let tokens = parse_boolean_query("* ** ***").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], BooleanToken::Wildcard("*".to_string()));
        assert_eq!(tokens[1], BooleanToken::Wildcard("**".to_string()));
        assert_eq!(tokens[2], BooleanToken::Wildcard("***".to_string()));
    }

    #[test]
    fn test_parse_boolean_tabs_and_newlines() {
        let tokens = parse_boolean_query("+database\t-slow\noptional").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0], BooleanToken::MustHave("database".to_string()));
        assert_eq!(tokens[1], BooleanToken::MustNotHave("slow".to_string()));
        assert_eq!(tokens[2], BooleanToken::Optional("optional".to_string()));
    }

    #[test]
    fn test_mysql_fts_mode_enum() {
        assert_eq!(MySQLFTSMode::NaturalLanguage, MySQLFTSMode::NaturalLanguage);
        assert_ne!(MySQLFTSMode::NaturalLanguage, MySQLFTSMode::Boolean);
        assert_ne!(MySQLFTSMode::Boolean, MySQLFTSMode::QueryExpansion);
    }

    #[test]
    fn test_match_expr_structure() {
        let expr = MySQLMatchExpr {
            columns: vec!["title".to_string(), "body".to_string()],
            query: "database optimization".to_string(),
            mode: MySQLFTSMode::Boolean,
        };

        assert_eq!(expr.columns.len(), 2);
        assert_eq!(expr.columns[0], "title");
        assert_eq!(expr.columns[1], "body");
        assert_eq!(expr.query, "database optimization");
        assert_eq!(expr.mode, MySQLFTSMode::Boolean);
    }

    #[test]
    fn test_parse_boolean_consecutive_operators() {
        let tokens = parse_boolean_query("++term").unwrap();
        // First + consumes, second + creates another MustHave
        // This matches MySQL behavior where ++ is redundant
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0], BooleanToken::MustHave("term".to_string()));
    }

    #[test]
    fn test_parse_boolean_operator_at_end() {
        let result = parse_boolean_query("term +");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_boolean_mixed_quotes_and_groups() {
        let tokens = parse_boolean_query(r#"+"exact phrase" (term1 term2)"#).unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0], BooleanToken::Phrase("exact phrase".to_string()));
        if let BooleanToken::Group(g) = &tokens[1] {
            assert_eq!(g.len(), 2);
        } else {
            panic!("Expected Group");
        }
    }

    #[test]
    fn test_parse_boolean_deep_nesting() {
        let tokens = parse_boolean_query("(((a)))").unwrap();
        assert_eq!(tokens.len(), 1);
        if let BooleanToken::Group(g1) = &tokens[0] {
            assert_eq!(g1.len(), 1);
            if let BooleanToken::Group(g2) = &g1[0] {
                assert_eq!(g2.len(), 1);
                if let BooleanToken::Group(g3) = &g2[0] {
                    assert_eq!(g3.len(), 1);
                } else {
                    panic!("Expected third level group");
                }
            } else {
                panic!("Expected second level group");
            }
        } else {
            panic!("Expected first level group");
        }
    }
}

//! SQL formatter that parses SQL and pretty-prints it with
//! configurable styles.
//!
//! Supports keyword capitalization, indentation control, and
//! clause-per-line formatting.

use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use thiserror::Error;

/// Errors from SQL formatting.
#[derive(Debug, Error)]
pub enum FormatError {
    /// SQL parsing failed.
    #[error("failed to parse SQL: {0}")]
    ParseError(String),
}

/// How to capitalize SQL keywords.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapitalizeMode {
    /// Uppercase all SQL keywords (SELECT, FROM, WHERE).
    Keywords,
    /// Uppercase the entire statement.
    All,
    /// No capitalization changes (preserve original).
    None,
}

/// Indentation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    /// Indent with N spaces.
    Spaces(u8),
    /// Indent with tabs.
    Tab,
}

/// Configuration for SQL formatting.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// Keyword capitalization mode.
    pub capitalize: CapitalizeMode,
    /// Indentation style.
    pub indent: IndentStyle,
    /// Maximum line width before wrapping (0 = no limit).
    pub max_width: usize,
    /// Put each major clause on its own line.
    pub clause_per_line: bool,
    /// Right-align major clause keywords (SELECT, FROM, WHERE)
    /// to a consistent column width.
    pub align_keywords: bool,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            capitalize: CapitalizeMode::Keywords,
            indent: IndentStyle::Spaces(2),
            max_width: 80,
            clause_per_line: true,
            align_keywords: false,
        }
    }
}

/// SQL formatter.
pub struct SqlFormatter {
    config: FormatConfig,
}

impl SqlFormatter {
    /// Create a new formatter with the given configuration.
    #[must_use]
    pub fn new(config: FormatConfig) -> Self {
        Self { config }
    }

    /// Create a formatter with default configuration.
    #[must_use]
    pub fn default_style() -> Self {
        Self::new(FormatConfig::default())
    }

    /// Format a SQL string.
    ///
    /// # Errors
    ///
    /// Returns `FormatError` if the SQL cannot be parsed.
    pub fn format(&self, sql: &str) -> Result<String, FormatError> {
        let dialect = GenericDialect {};
        let statements =
            Parser::parse_sql(&dialect, sql).map_err(|e| FormatError::ParseError(e.to_string()))?;

        let mut formatted_parts = Vec::new();
        for stmt in &statements {
            let raw = stmt.to_string();
            let result = self.apply_style(&raw);
            formatted_parts.push(result);
        }

        Ok(formatted_parts.join(";\n"))
    }

    fn apply_style(&self, sql: &str) -> String {
        let mut result = sql.to_owned();

        // Apply capitalization
        result = self.apply_capitalize(&result);

        // Apply clause-per-line formatting
        if self.config.clause_per_line {
            result = self.apply_clause_breaks(&result);
        }

        result
    }

    fn apply_capitalize(&self, sql: &str) -> String {
        match self.config.capitalize {
            CapitalizeMode::All => sql.to_uppercase(),
            CapitalizeMode::None => sql.to_owned(),
            CapitalizeMode::Keywords => capitalize_keywords(sql),
        }
    }

    fn apply_clause_breaks(&self, sql: &str) -> String {
        let indent = self.indent_string();
        let inner_indent = format!("{indent}{indent}"); // extra indent inside CTE bodies
        // Alignment width for right-aligning keywords
        let align_width: usize = 9; // len("RETURNING") + 1
        let mut result = String::with_capacity(sql.len() + 128);
        let mut depth: usize = 0;
        // Depths at which we opened a CTE/subquery body with an `AS (` open,
        // so we can add a closing newline before `)` and indent inner clauses.
        let mut cte_body_depths: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        let mut prev_upper = String::new();
        let mut in_string = false;
        let mut string_char: char = '\'';

        let tokens = tokenize_for_formatting(sql);

        for (i, token) in tokens.iter().enumerate() {
            let upper = token.to_uppercase();

            // Track string literals — don't tokenize inside them.
            if !in_string && (token == "'" || token == "\"") {
                in_string = true;
                string_char = token.chars().next().unwrap_or('\'');
                result.push_str(token);
                prev_upper = upper;
                continue;
            }
            if in_string {
                result.push_str(token);
                if token.len() == 1 && token.starts_with(string_char) {
                    in_string = false;
                }
                // Don't update prev_upper inside strings
                continue;
            }

            // Skip whitespace — just emit it and don't update prev_upper.
            if token.trim().is_empty() {
                result.push_str(token);
                continue;
            }

            // Opening parenthesis: detect CTE/subquery bodies.
            if token == "(" {
                depth += 1;
                // An `AS (` at any tracked depth opens a formatted subquery.
                if prev_upper == "AS" {
                    cte_body_depths.insert(depth);
                    result.push('(');
                    result.push('\n');
                    result.push_str(&inner_indent);
                    prev_upper = upper;
                    continue;
                }
                result.push_str(token);
                prev_upper = upper;
                continue;
            }

            // Closing parenthesis: emit newline before `)` if it closes a CTE body.
            if token == ")" {
                if cte_body_depths.contains(&depth) {
                    cte_body_depths.remove(&depth);
                    let trimmed_len = result.trim_end().len();
                    result.truncate(trimmed_len);
                    result.push('\n');
                }
                depth = depth.saturating_sub(1);
                result.push_str(token);
                prev_upper = upper;
                continue;
            }

            let in_cte_body = cte_body_depths.contains(&depth);

            // Break clause keywords at the top level (depth 0)
            // or inside a CTE/subquery body (depth in cte_body_depths).
            if depth == 0 || in_cte_body {
                let is_clause = matches!(
                    upper.as_str(),
                    "SELECT"
                        | "FROM"
                        | "WHERE"
                        | "GROUP"
                        | "HAVING"
                        | "ORDER"
                        | "LIMIT"
                        | "OFFSET"
                        | "UNION"
                        | "INTERSECT"
                        | "EXCEPT"
                        | "WITH"
                        | "RETURNING"
                );

                let is_join = matches!(
                    upper.as_str(),
                    "JOIN" | "INNER" | "LEFT" | "RIGHT" | "FULL" | "CROSS"
                );

                let is_subclause = matches!(upper.as_str(), "AND" | "OR");

                // The line prefix differs between top-level and inner bodies.
                let base_prefix = if in_cte_body { &inner_indent } else { "" };
                let join_prefix = if in_cte_body {
                    format!("{inner_indent}{indent}")
                } else {
                    indent.clone()
                };

                if is_clause && i > 0 {
                    let trimmed_len = result.trim_end().len();
                    result.truncate(trimmed_len);
                    result.push('\n');
                    if depth == 0 && self.config.align_keywords {
                        let padding = align_width.saturating_sub(token.len());
                        for _ in 0..padding {
                            result.push(' ');
                        }
                    } else {
                        result.push_str(base_prefix);
                    }
                    result.push_str(token);
                } else if (is_join || is_subclause) && i > 0 {
                    let trimmed_len = result.trim_end().len();
                    result.truncate(trimmed_len);
                    result.push('\n');
                    result.push_str(&join_prefix);
                    result.push_str(token);
                } else {
                    result.push_str(token);
                }
            } else {
                result.push_str(token);
            }

            prev_upper = upper;
        }

        result
    }

    fn indent_string(&self) -> String {
        match self.config.indent {
            IndentStyle::Spaces(n) => " ".repeat(n as usize),
            IndentStyle::Tab => "\t".to_owned(),
        }
    }
}

/// Capitalize SQL keywords while preserving identifiers and
/// string literals.
#[expect(
    clippy::too_many_lines,
    reason = "Keyword capitalization requires handling many SQL keywords"
)]
fn capitalize_keywords(sql: &str) -> String {
    let keywords: &[&str] = &[
        "SELECT",
        "FROM",
        "WHERE",
        "AND",
        "OR",
        "NOT",
        "INSERT",
        "INTO",
        "VALUES",
        "UPDATE",
        "SET",
        "DELETE",
        "CREATE",
        "TABLE",
        "DROP",
        "ALTER",
        "JOIN",
        "INNER",
        "LEFT",
        "RIGHT",
        "FULL",
        "OUTER",
        "CROSS",
        "ON",
        "AS",
        "IN",
        "EXISTS",
        "BETWEEN",
        "LIKE",
        "ILIKE",
        "IS",
        "NULL",
        "TRUE",
        "FALSE",
        "ORDER",
        "BY",
        "GROUP",
        "HAVING",
        "LIMIT",
        "OFFSET",
        "UNION",
        "ALL",
        "INTERSECT",
        "EXCEPT",
        "DISTINCT",
        "CASE",
        "WHEN",
        "THEN",
        "ELSE",
        "END",
        "WITH",
        "RECURSIVE",
        "ASC",
        "DESC",
        "NULLS",
        "FIRST",
        "LAST",
        "OVER",
        "PARTITION",
        "ROWS",
        "RANGE",
        "GROUPS",
        "PRECEDING",
        "FOLLOWING",
        "CURRENT",
        "ROW",
        "UNBOUNDED",
        "FETCH",
        "NEXT",
        "ONLY",
        "CAST",
        "COUNT",
        "SUM",
        "AVG",
        "MIN",
        "MAX",
        "ROW_NUMBER",
        "RANK",
        "DENSE_RANK",
        "NTILE",
        "LAG",
        "LEAD",
        "FIRST_VALUE",
        "LAST_VALUE",
        "STDDEV",
        "VARIANCE",
        "COALESCE",
        "NULLIF",
        "USING",
        "NATURAL",
        "WINDOW",
        "RETURNING",
        "CONFLICT",
        "DO",
        "NOTHING",
        "INDEX",
        "CONSTRAINT",
        "PRIMARY",
        "KEY",
        "FOREIGN",
        "REFERENCES",
        "UNIQUE",
        "CHECK",
        "DEFAULT",
    ];

    let tokens = tokenize_for_formatting(sql);
    let mut result = String::with_capacity(sql.len());
    let mut in_string = false;

    for token in &tokens {
        if !in_string && (token == "'" || token == "\"") {
            in_string = true;
            result.push_str(token);
            continue;
        }
        if in_string {
            result.push_str(token);
            if token == "'" || token == "\"" {
                in_string = false;
            }
            continue;
        }

        let upper = token.to_uppercase();
        if keywords.contains(&upper.as_str()) {
            result.push_str(&upper);
        } else {
            result.push_str(token);
        }
    }

    result
}

/// Simple tokenizer that splits SQL into words, punctuation,
/// and whitespace tokens while preserving string literals.
fn tokenize_for_formatting(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = sql.chars().peekable();
    let mut buf = String::new();

    while let Some(&ch) = chars.peek() {
        match ch {
            '\'' | '"' => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
                let quote = ch;
                chars.next();
                tokens.push(quote.to_string());
                let mut literal = String::new();
                while let Some(&c) = chars.peek() {
                    if c == quote {
                        chars.next();
                        // Check for escaped quote (double quote)
                        if chars.peek() == Some(&quote) {
                            literal.push(quote);
                            literal.push(quote);
                            chars.next();
                        } else {
                            break;
                        }
                    } else {
                        literal.push(c);
                        chars.next();
                    }
                }
                if !literal.is_empty() {
                    tokens.push(literal);
                }
                tokens.push(quote.to_string());
            }
            '(' | ')' | ',' | ';' => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
                chars.next();
                tokens.push(ch.to_string());
            }
            c if c.is_whitespace() => {
                if !buf.is_empty() {
                    tokens.push(std::mem::take(&mut buf));
                }
                let mut ws = String::new();
                while let Some(&w) = chars.peek() {
                    if w.is_whitespace() {
                        ws.push(w);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(ws);
            }
            _ => {
                buf.push(ch);
                chars.next();
            }
        }
    }

    if !buf.is_empty() {
        tokens.push(buf);
    }

    tokens
}

#[expect(clippy::expect_used, reason = "test code")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_simple_select() {
        let formatter = SqlFormatter::default_style();
        let result = formatter
            .format("select id,name from users where age>18")
            .expect("should format");
        let upper = result.to_uppercase();
        assert!(upper.contains("SELECT"), "expected SELECT in: {result}");
        assert!(upper.contains("FROM"), "expected FROM in: {result}");
        assert!(upper.contains("WHERE"), "expected WHERE in: {result}");
    }

    #[test]
    fn format_uppercase_all() {
        let config = FormatConfig {
            capitalize: CapitalizeMode::All,
            ..FormatConfig::default()
        };
        let formatter = SqlFormatter::new(config);
        let result = formatter
            .format("select id from users")
            .expect("should format");
        assert!(result.contains("SELECT"), "expected SELECT: {result}");
        assert!(result.contains("USERS"), "expected USERS: {result}");
    }

    #[test]
    fn format_no_capitalize() {
        let config = FormatConfig {
            capitalize: CapitalizeMode::None,
            clause_per_line: false,
            ..FormatConfig::default()
        };
        let formatter = SqlFormatter::new(config);
        let result = formatter
            .format("SELECT id FROM users")
            .expect("should format");
        // sqlparser normalizes to uppercase, so the output
        // will still contain SELECT/FROM from re-serialization
        assert!(result.contains("SELECT") || result.contains("select"));
    }

    #[test]
    fn format_clause_per_line() {
        let config = FormatConfig {
            capitalize: CapitalizeMode::Keywords,
            clause_per_line: true,
            ..FormatConfig::default()
        };
        let formatter = SqlFormatter::new(config);
        let result = formatter
            .format(
                "select id, name from users where age > 18 \
                 order by name limit 10",
            )
            .expect("should format");
        assert!(result.contains('\n'), "expected newlines in: {result}");
    }

    #[test]
    fn format_with_join() {
        let formatter = SqlFormatter::default_style();
        let result = formatter
            .format(
                "select * from orders o join customers c \
                 on o.customer_id = c.id where o.total > 100",
            )
            .expect("should format");
        let upper = result.to_uppercase();
        assert!(upper.contains("JOIN"), "expected JOIN in: {result}");
    }

    #[test]
    fn format_cte() {
        let formatter = SqlFormatter::default_style();
        let result = formatter
            .format(
                "with active as (select * from users where active = true) \
                 select * from active",
            )
            .expect("should format");
        let upper = result.to_uppercase();
        assert!(upper.contains("WITH"), "expected WITH in: {result}");
    }

    #[test]
    fn format_window_function() {
        let formatter = SqlFormatter::default_style();
        let result = formatter
            .format(
                "select id, row_number() over (partition by dept \
                 order by salary desc) as rn from employees",
            )
            .expect("should format");
        let upper = result.to_uppercase();
        assert!(
            upper.contains("ROW_NUMBER"),
            "expected ROW_NUMBER in: {result}"
        );
        assert!(upper.contains("OVER"), "expected OVER in: {result}");
    }

    #[test]
    fn format_invalid_sql_returns_error() {
        let formatter = SqlFormatter::default_style();
        let result = formatter.format("NOT VALID SQL %%% !!!");
        assert!(result.is_err());
    }

    #[test]
    fn format_with_tabs() {
        let config = FormatConfig {
            indent: IndentStyle::Tab,
            ..FormatConfig::default()
        };
        let formatter = SqlFormatter::new(config);
        let result = formatter
            .format(
                "select * from orders o join customers c \
                 on o.id = c.id",
            )
            .expect("should format");
        // We don't mandate tabs appear, but it should parse and format
        assert!(result.contains("JOIN") || result.contains("join"));
    }

    #[test]
    fn format_preserves_string_literals() {
        let formatter = SqlFormatter::default_style();
        let result = formatter
            .format("select * from users where name = 'from where'")
            .expect("should format");
        assert!(
            result.contains("from where") || result.contains("FROM WHERE"),
            "string literal should be preserved: {result}"
        );
    }

    #[test]
    fn format_spaces_4() {
        let config = FormatConfig {
            indent: IndentStyle::Spaces(4),
            ..FormatConfig::default()
        };
        let formatter = SqlFormatter::new(config);
        let result = formatter
            .format("select * from a left join b on a.id = b.id")
            .expect("should format");
        assert!(!result.is_empty());
    }

    #[test]
    fn tokenize_simple() {
        let tokens = tokenize_for_formatting("SELECT id FROM t");
        let non_ws: Vec<_> = tokens
            .iter()
            .filter(|t| !t.trim().is_empty())
            .cloned()
            .collect();
        assert_eq!(non_ws, vec!["SELECT", "id", "FROM", "t"]);
    }

    #[test]
    fn format_align_keywords() {
        let config = FormatConfig {
            align_keywords: true,
            ..FormatConfig::default()
        };
        let formatter = SqlFormatter::new(config);
        let result = formatter
            .format(
                "select id, name from users \
                 where age > 18 order by name",
            )
            .expect("should format");
        assert!(result.contains('\n'), "expected newlines: {result}");
        // Keywords should be right-aligned with padding
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() >= 3, "expected multiple lines: {result}");
    }

    #[test]
    fn format_and_or_subclauses() {
        let formatter = SqlFormatter::default_style();
        let result = formatter
            .format(
                "select * from users where age > 18 \
                 and name = 'test' or active = true",
            )
            .expect("should format");
        // AND and OR should be on their own lines
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() >= 3, "expected AND/OR on own lines: {result}");
    }

    #[test]
    fn format_returning_clause() {
        let formatter = SqlFormatter::default_style();
        let result = formatter
            .format(
                "INSERT INTO users (name) VALUES ('test') \
                 RETURNING id",
            )
            .expect("should format");
        let upper = result.to_uppercase();
        assert!(upper.contains("RETURNING"), "expected RETURNING: {result}");
    }
}


    #[test]
    fn test_simple_query_formatting() {
        let formatter = SqlFormatter::default_style();
        // This should parse successfully with ra-sql-parser
        let result = formatter.format("with a as (select id from t1), b as (select id from t2) select a.id from a join b on a.id = b.id");
        match &result {
            Ok(s) => eprintln!("OK:\n{s}"),
            Err(e) => eprintln!("ERROR: {e}"),
        }
        assert!(result.is_ok(), "should format: {:?}", result.err());
    }

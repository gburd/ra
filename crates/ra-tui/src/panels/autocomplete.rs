//! SQL auto-completion with fuzzy matching.
//!
//! Provides context-aware completion for SQL keywords,
//! functions, and common table/column names. Uses
//! `fuzzy-matcher` for scored ranking.

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

/// What kind of token the cursor is positioned at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionKind {
    /// SQL keyword position (start of clause, after FROM, etc.).
    Keyword,
    /// Function name position (after SELECT, in expressions).
    Function,
    /// Table name position (after FROM, JOIN).
    Table,
    /// Column name position (in SELECT list, WHERE clause).
    Column,
}

/// Context for a completion request.
#[derive(Debug, Clone)]
pub struct CompletionContext {
    /// The partial text to match against.
    pub prefix: String,
    /// What kind of completion to provide.
    pub kind: CompletionKind,
}

impl CompletionContext {
    /// Detect the completion context from the cursor position.
    #[must_use]
    pub fn detect(
        line: &str,
        col: usize,
        _full_text: &str,
    ) -> Self {
        let before = &line[..col.min(line.len())];
        let word_start = before
            .rfind(|c: char| c.is_whitespace() || c == '(')
            .map_or(0, |p| p + 1);
        let prefix = before[word_start..].to_owned();

        let preceding = before[..word_start]
            .trim_end()
            .to_uppercase();

        let kind = if preceding.ends_with("FROM")
            || preceding.ends_with("JOIN")
            || preceding.ends_with("INTO")
            || preceding.ends_with("UPDATE")
            || preceding.ends_with("TABLE")
        {
            CompletionKind::Table
        } else if preceding.ends_with("SELECT")
            || preceding.ends_with(',')
            || preceding.ends_with("ON")
            || preceding.ends_with("WHERE")
            || preceding.ends_with("AND")
            || preceding.ends_with("OR")
            || preceding.ends_with("BY")
            || preceding.ends_with("HAVING")
            || preceding.ends_with("SET")
        {
            CompletionKind::Column
        } else if prefix.is_empty()
            || preceding.is_empty()
        {
            CompletionKind::Keyword
        } else {
            // Default: offer both keywords and functions
            CompletionKind::Function
        };

        Self { prefix, kind }
    }
}

/// SQL auto-completer with fuzzy matching.
pub struct SqlCompleter {
    matcher: SkimMatcherV2,
    keywords: Vec<&'static str>,
    functions: Vec<&'static str>,
    sample_tables: Vec<&'static str>,
    sample_columns: Vec<&'static str>,
}

impl std::fmt::Debug for SqlCompleter {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("SqlCompleter")
            .field("keywords", &self.keywords.len())
            .field("functions", &self.functions.len())
            .field("tables", &self.sample_tables.len())
            .field("columns", &self.sample_columns.len())
            .finish_non_exhaustive()
    }
}

impl Clone for SqlCompleter {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl SqlCompleter {
    /// Create a new completer with built-in SQL vocabulary.
    #[must_use]
    pub fn new() -> Self {
        Self {
            matcher: SkimMatcherV2::default(),
            keywords: sql_keywords(),
            functions: sql_functions(),
            sample_tables: sample_tables(),
            sample_columns: sample_columns(),
        }
    }

    /// Get ranked completions for the given context.
    pub fn complete(
        &self,
        context: &CompletionContext,
        limit: usize,
    ) -> Vec<String> {
        if context.prefix.is_empty() {
            return Vec::new();
        }

        let candidates = match context.kind {
            CompletionKind::Keyword => &self.keywords,
            CompletionKind::Function => &self.functions,
            CompletionKind::Table => &self.sample_tables,
            CompletionKind::Column => &self.sample_columns,
        };

        let mut scored: Vec<(i64, &str)> = candidates
            .iter()
            .filter_map(|candidate| {
                self.matcher
                    .fuzzy_match(candidate, &context.prefix)
                    .map(|score| (score, *candidate))
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));

        scored
            .into_iter()
            .take(limit)
            .map(|(_, s)| s.to_owned())
            .collect()
    }
}

impl Default for SqlCompleter {
    fn default() -> Self {
        Self::new()
    }
}

fn sql_keywords() -> Vec<&'static str> {
    vec![
        "SELECT", "FROM", "WHERE", "AND", "OR", "NOT",
        "INSERT", "INTO", "VALUES", "UPDATE", "SET",
        "DELETE", "CREATE", "TABLE", "DROP", "ALTER",
        "JOIN", "INNER", "LEFT", "RIGHT", "FULL",
        "OUTER", "CROSS", "ON", "AS", "IN", "EXISTS",
        "BETWEEN", "LIKE", "ILIKE", "IS", "NULL",
        "TRUE", "FALSE", "ORDER", "BY", "GROUP",
        "HAVING", "LIMIT", "OFFSET", "UNION", "ALL",
        "INTERSECT", "EXCEPT", "DISTINCT", "CASE",
        "WHEN", "THEN", "ELSE", "END", "WITH",
        "RECURSIVE", "ASC", "DESC", "NULLS", "FIRST",
        "LAST", "OVER", "PARTITION", "ROWS", "RANGE",
        "GROUPS", "PRECEDING", "FOLLOWING", "CURRENT",
        "ROW", "UNBOUNDED", "FETCH", "NEXT", "ONLY",
        "RETURNING", "USING", "NATURAL", "WINDOW",
        "INDEX", "CONSTRAINT", "PRIMARY", "KEY",
        "FOREIGN", "REFERENCES", "UNIQUE", "CHECK",
        "DEFAULT", "CASCADE", "RESTRICT", "VIEW",
        "TRIGGER", "GRANT", "REVOKE",
    ]
}

fn sql_functions() -> Vec<&'static str> {
    vec![
        "COUNT", "SUM", "AVG", "MIN", "MAX",
        "ROW_NUMBER", "RANK", "DENSE_RANK", "NTILE",
        "LAG", "LEAD", "FIRST_VALUE", "LAST_VALUE",
        "NTH_VALUE", "STDDEV", "VARIANCE",
        "COALESCE", "NULLIF", "CAST",
        "UPPER", "LOWER", "TRIM", "LTRIM", "RTRIM",
        "LENGTH", "CHAR_LENGTH", "SUBSTRING", "REPLACE",
        "CONCAT", "POSITION", "OVERLAY",
        "ABS", "CEIL", "FLOOR", "ROUND", "MOD",
        "POWER", "SQRT", "LOG", "LN", "EXP",
        "NOW", "CURRENT_TIMESTAMP", "CURRENT_DATE",
        "CURRENT_TIME", "EXTRACT", "DATE_TRUNC",
        "DATE_PART", "AGE", "INTERVAL",
        "ARRAY_AGG", "STRING_AGG", "LISTAGG",
        "JSON_AGG", "JSONB_AGG",
        "GREATEST", "LEAST",
        "GENERATE_SERIES", "UNNEST",
        "EXISTS", "ANY", "SOME",
    ]
}

fn sample_tables() -> Vec<&'static str> {
    vec![
        "users", "orders", "products", "customers",
        "employees", "departments", "categories",
        "inventory", "transactions", "accounts",
        "sessions", "events", "logs", "payments",
        "addresses", "reviews", "comments", "tags",
        "roles", "permissions",
    ]
}

fn sample_columns() -> Vec<&'static str> {
    vec![
        "id", "name", "email", "created_at", "updated_at",
        "status", "type", "price", "quantity", "total",
        "description", "title", "user_id", "order_id",
        "product_id", "customer_id", "department_id",
        "first_name", "last_name", "phone", "address",
        "city", "state", "country", "zip_code",
        "active", "deleted", "score", "rating",
        "amount", "balance", "date", "timestamp",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_keyword_sel() {
        let completer = SqlCompleter::new();
        let ctx = CompletionContext {
            prefix: "SEL".to_owned(),
            kind: CompletionKind::Keyword,
        };
        let results = completer.complete(&ctx, 10);
        assert!(
            results.contains(&"SELECT".to_owned()),
            "expected SELECT in {:?}",
            results
        );
    }

    #[test]
    fn complete_function_cou() {
        let completer = SqlCompleter::new();
        let ctx = CompletionContext {
            prefix: "COU".to_owned(),
            kind: CompletionKind::Function,
        };
        let results = completer.complete(&ctx, 10);
        assert!(
            results.contains(&"COUNT".to_owned()),
            "expected COUNT in {:?}",
            results
        );
    }

    #[test]
    fn complete_table_use() {
        let completer = SqlCompleter::new();
        let ctx = CompletionContext {
            prefix: "use".to_owned(),
            kind: CompletionKind::Table,
        };
        let results = completer.complete(&ctx, 10);
        assert!(
            results.contains(&"users".to_owned()),
            "expected users in {:?}",
            results
        );
    }

    #[test]
    fn complete_column_nam() {
        let completer = SqlCompleter::new();
        let ctx = CompletionContext {
            prefix: "nam".to_owned(),
            kind: CompletionKind::Column,
        };
        let results = completer.complete(&ctx, 10);
        assert!(
            results.contains(&"name".to_owned()),
            "expected name in {:?}",
            results
        );
    }

    #[test]
    fn empty_prefix_returns_nothing() {
        let completer = SqlCompleter::new();
        let ctx = CompletionContext {
            prefix: String::new(),
            kind: CompletionKind::Keyword,
        };
        let results = completer.complete(&ctx, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn respects_limit() {
        let completer = SqlCompleter::new();
        let ctx = CompletionContext {
            prefix: "S".to_owned(),
            kind: CompletionKind::Keyword,
        };
        let results = completer.complete(&ctx, 3);
        assert!(results.len() <= 3);
    }

    #[test]
    fn detect_keyword_context() {
        let ctx = CompletionContext::detect(
            "SEL",
            3,
            "SEL",
        );
        assert_eq!(ctx.prefix, "SEL");
        assert_eq!(ctx.kind, CompletionKind::Keyword);
    }

    #[test]
    fn detect_table_context_after_from() {
        let ctx = CompletionContext::detect(
            "SELECT * FROM us",
            16,
            "SELECT * FROM us",
        );
        assert_eq!(ctx.prefix, "us");
        assert_eq!(ctx.kind, CompletionKind::Table);
    }

    #[test]
    fn detect_column_context_after_select() {
        let ctx = CompletionContext::detect(
            "SELECT na",
            9,
            "SELECT na",
        );
        assert_eq!(ctx.prefix, "na");
        assert_eq!(ctx.kind, CompletionKind::Column);
    }

    #[test]
    fn detect_column_context_after_where() {
        let ctx = CompletionContext::detect(
            "SELECT * FROM t WHERE na",
            24,
            "SELECT * FROM t WHERE na",
        );
        assert_eq!(ctx.prefix, "na");
        assert_eq!(ctx.kind, CompletionKind::Column);
    }

    #[test]
    fn fuzzy_match_partial() {
        let completer = SqlCompleter::new();
        let ctx = CompletionContext {
            prefix: "SLCT".to_owned(),
            kind: CompletionKind::Keyword,
        };
        let results = completer.complete(&ctx, 10);
        assert!(
            results.contains(&"SELECT".to_owned()),
            "fuzzy match should find SELECT for SLCT: {:?}",
            results
        );
    }

    #[test]
    fn completer_default() {
        let completer = SqlCompleter::default();
        let ctx = CompletionContext {
            prefix: "SEL".to_owned(),
            kind: CompletionKind::Keyword,
        };
        let results = completer.complete(&ctx, 5);
        assert!(!results.is_empty());
    }
}

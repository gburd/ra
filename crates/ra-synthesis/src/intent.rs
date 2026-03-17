//! Intent parsing from natural language queries.
//!
//! Extracts structured [`QueryIntent`] from free-form text using
//! keyword and phrase matching against a known schema.

use crate::error::SynthesisError;
use crate::schema::SchemaInfo;
use serde::{Deserialize, Serialize};

/// Parsed intent from a natural language query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryIntent {
    /// Tables referenced in the query.
    pub tables: Vec<String>,
    /// Columns to select (empty = all columns).
    pub select_columns: Vec<ColumnIntent>,
    /// Filter conditions extracted from the query.
    pub filters: Vec<FilterIntent>,
    /// Aggregate operations.
    pub aggregates: Vec<AggregateIntent>,
    /// GROUP BY columns.
    pub group_by: Vec<String>,
    /// Sort specification.
    pub order_by: Vec<OrderIntent>,
    /// Row limit.
    pub limit: Option<u64>,
    /// Join hints extracted from the query.
    pub joins: Vec<JoinIntent>,
}

/// A column selection with optional table qualifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnIntent {
    /// Column name.
    pub column: String,
    /// Optional table qualifier.
    pub table: Option<String>,
}

/// A filter condition parsed from natural language.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterIntent {
    /// Column to filter on.
    pub column: String,
    /// Comparison operator.
    pub op: FilterOp,
    /// The literal value to compare against.
    pub value: String,
}

/// Filter comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOp {
    /// Equality.
    Eq,
    /// Not equal.
    Ne,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// String contains / LIKE.
    Like,
}

/// An aggregate operation parsed from the query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateIntent {
    /// Aggregate function name (count, sum, avg, min, max).
    pub function: String,
    /// Column to aggregate (None for COUNT(*)).
    pub column: Option<String>,
}

/// A sort directive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderIntent {
    /// Column to sort by.
    pub column: String,
    /// Sort direction.
    pub descending: bool,
}

/// A join hint between two tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinIntent {
    /// Left table.
    pub left_table: String,
    /// Right table.
    pub right_table: String,
}

/// Parser that extracts [`QueryIntent`] from natural language.
pub struct IntentParser<'a> {
    schema: &'a SchemaInfo,
}

impl<'a> IntentParser<'a> {
    /// Create a new parser for the given schema.
    #[must_use]
    pub fn new(schema: &'a SchemaInfo) -> Self {
        Self { schema }
    }

    /// Parse a natural language query into structured intent.
    ///
    /// # Errors
    ///
    /// Returns `SynthesisError::IntentParseFailed` if the input
    /// cannot be meaningfully interpreted, or
    /// `SynthesisError::NoTablesIdentified` if no schema tables
    /// are mentioned.
    pub fn parse(
        &self,
        input: &str,
    ) -> Result<QueryIntent, SynthesisError> {
        let normalized = normalize(input);
        let tokens: Vec<&str> = normalized.split_whitespace().collect();

        let tables = self.extract_tables(&normalized);
        if tables.is_empty() {
            return Err(SynthesisError::NoTablesIdentified);
        }

        let select_columns = self.extract_columns(&normalized, &tables);
        let filters = self.extract_filters(&tokens, &tables);
        let aggregates = extract_aggregates(&tokens);
        let group_by = extract_group_by(&aggregates, &select_columns);
        let order_by = extract_order_by(&tokens);
        let limit = extract_limit(&tokens);
        let joins = self.extract_joins(&tables);

        Ok(QueryIntent {
            tables,
            select_columns,
            filters,
            aggregates,
            group_by,
            order_by,
            limit,
            joins,
        })
    }

    fn extract_tables(&self, input: &str) -> Vec<String> {
        let mut found = Vec::new();
        for name in self.schema.table_names() {
            let lower = name.to_lowercase();
            if input.contains(&lower)
                || input.contains(&singularize(&lower))
            {
                found.push(name.to_string());
            }
        }
        found
    }

    fn extract_columns(
        &self,
        input: &str,
        tables: &[String],
    ) -> Vec<ColumnIntent> {
        let mut columns = Vec::new();
        for table_name in tables {
            if let Some(table) = self.schema.find_table(table_name) {
                for col in &table.columns {
                    let lower = col.name.to_lowercase();
                    let spaced =
                        lower.replace('_', " ");
                    if input.contains(&lower)
                        || input.contains(&spaced)
                    {
                        columns.push(ColumnIntent {
                            column: col.name.clone(),
                            table: Some(table.name.clone()),
                        });
                    }
                }
            }
        }
        columns
    }

    fn extract_filters(
        &self,
        tokens: &[&str],
        tables: &[String],
    ) -> Vec<FilterIntent> {
        let mut filters = Vec::new();

        let all_columns: Vec<(String, String)> = tables
            .iter()
            .filter_map(|t| self.schema.find_table(t))
            .flat_map(|t| {
                t.columns.iter().map(move |c| {
                    (c.name.to_lowercase(), c.name.clone())
                })
            })
            .collect();

        let patterns: &[(&[&str], FilterOp)] = &[
            (&["greater", "than"], FilterOp::Gt),
            (&["more", "than"], FilterOp::Gt),
            (&["above"], FilterOp::Gt),
            (&["over"], FilterOp::Gt),
            (&["at", "least"], FilterOp::Ge),
            (&["less", "than"], FilterOp::Lt),
            (&["under"], FilterOp::Lt),
            (&["below"], FilterOp::Lt),
            (&["at", "most"], FilterOp::Le),
            (&["equal", "to"], FilterOp::Eq),
            (&["equals"], FilterOp::Eq),
            (&["is"], FilterOp::Eq),
            (&["not"], FilterOp::Ne),
            (&["containing"], FilterOp::Like),
            (&["contains"], FilterOp::Like),
            (&["like"], FilterOp::Like),
        ];

        for (idx, token) in tokens.iter().enumerate() {
            let lower_tok = token.to_lowercase();
            for (col_lower, col_name) in &all_columns {
                let col_spaced = col_lower.replace('_', " ");
                if lower_tok != *col_lower
                    && !tokens_match_phrase(tokens, idx, &col_spaced)
                {
                    continue;
                }

                let rest_start = if tokens_match_phrase(
                    tokens,
                    idx,
                    &col_spaced,
                ) {
                    idx + col_spaced.split_whitespace().count()
                } else {
                    idx + 1
                };

                if let Some((op, value_start)) =
                    match_op_pattern(tokens, rest_start, patterns)
                {
                    if let Some(value) =
                        extract_value(tokens, value_start)
                    {
                        filters.push(FilterIntent {
                            column: col_name.clone(),
                            op,
                            value,
                        });
                    }
                }
            }
        }
        filters
    }

    fn extract_joins(&self, tables: &[String]) -> Vec<JoinIntent> {
        let mut joins = Vec::new();
        if tables.len() < 2 {
            return joins;
        }

        for table_name in tables {
            if let Some(table) = self.schema.find_table(table_name) {
                for fk in &table.foreign_keys {
                    if tables.iter().any(|t| {
                        t.eq_ignore_ascii_case(
                            &fk.referenced_table,
                        )
                    }) {
                        joins.push(JoinIntent {
                            left_table: table.name.clone(),
                            right_table: fk
                                .referenced_table
                                .clone(),
                        });
                    }
                }
            }
        }
        joins
    }
}

fn normalize(input: &str) -> String {
    input
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != ' ', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn singularize(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        format!("{stem}y")
    } else if let Some(stem) = word.strip_suffix("ses") {
        stem.to_string()
    } else if let Some(stem) = word.strip_suffix('s') {
        stem.to_string()
    } else {
        word.to_string()
    }
}

fn tokens_match_phrase(
    tokens: &[&str],
    start: usize,
    phrase: &str,
) -> bool {
    let phrase_tokens: Vec<&str> =
        phrase.split_whitespace().collect();
    if start + phrase_tokens.len() > tokens.len() {
        return false;
    }
    phrase_tokens.iter().enumerate().all(|(i, pt)| {
        tokens[start + i].eq_ignore_ascii_case(pt)
    })
}

fn match_op_pattern(
    tokens: &[&str],
    start: usize,
    patterns: &[(&[&str], FilterOp)],
) -> Option<(FilterOp, usize)> {
    for &(phrase, op) in patterns {
        if start + phrase.len() <= tokens.len() {
            let matches = phrase.iter().enumerate().all(|(i, word)| {
                tokens[start + i].eq_ignore_ascii_case(word)
            });
            if matches {
                return Some((op, start + phrase.len()));
            }
        }
    }
    None
}

fn extract_value(tokens: &[&str], start: usize) -> Option<String> {
    if start >= tokens.len() {
        return None;
    }
    let mut parts = Vec::new();
    for token in &tokens[start..] {
        if is_stop_word(token) {
            break;
        }
        parts.push(*token);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn is_stop_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "and" | "or" | "but" | "then" | "sorted" | "ordered"
            | "grouped" | "limit" | "top"
    )
}

fn extract_aggregates(tokens: &[&str]) -> Vec<AggregateIntent> {
    let mut aggs = Vec::new();
    let agg_keywords = [
        ("count", "count"),
        ("number", "count"),
        ("total", "sum"),
        ("sum", "sum"),
        ("average", "avg"),
        ("avg", "avg"),
        ("mean", "avg"),
        ("minimum", "min"),
        ("min", "min"),
        ("lowest", "min"),
        ("smallest", "min"),
        ("maximum", "max"),
        ("max", "max"),
        ("highest", "max"),
        ("largest", "max"),
    ];

    for (idx, token) in tokens.iter().enumerate() {
        let lower = token.to_lowercase();
        for &(keyword, function) in &agg_keywords {
            if lower == keyword {
                let col = if idx + 2 < tokens.len()
                    && tokens[idx + 1].eq_ignore_ascii_case("of")
                {
                    Some(tokens[idx + 2].to_string())
                } else if idx + 1 < tokens.len()
                    && !is_stop_word(tokens[idx + 1])
                    && !tokens[idx + 1].eq_ignore_ascii_case("of")
                    && !tokens[idx + 1].eq_ignore_ascii_case("by")
                    && !tokens[idx + 1].eq_ignore_ascii_case("the")
                {
                    Some(tokens[idx + 1].to_string())
                } else {
                    None
                };
                aggs.push(AggregateIntent {
                    function: function.to_string(),
                    column: col,
                });
                break;
            }
        }
    }
    aggs
}

fn extract_group_by(
    aggregates: &[AggregateIntent],
    columns: &[ColumnIntent],
) -> Vec<String> {
    if aggregates.is_empty() {
        return Vec::new();
    }
    let agg_cols: Vec<&str> = aggregates
        .iter()
        .filter_map(|a| a.column.as_deref())
        .collect();
    columns
        .iter()
        .filter(|c| {
            !agg_cols
                .iter()
                .any(|ac| ac.eq_ignore_ascii_case(&c.column))
        })
        .map(|c| c.column.clone())
        .collect()
}

fn extract_order_by(tokens: &[&str]) -> Vec<OrderIntent> {
    let mut orders = Vec::new();
    let order_keywords = [
        "sorted", "ordered", "order", "sort",
    ];

    for (idx, token) in tokens.iter().enumerate() {
        let lower = token.to_lowercase();
        if !order_keywords.contains(&lower.as_str()) {
            continue;
        }
        let mut pos = idx + 1;
        if pos < tokens.len()
            && tokens[pos].eq_ignore_ascii_case("by")
        {
            pos += 1;
        }
        if pos < tokens.len() {
            let col = tokens[pos].to_string();
            let descending = tokens.get(pos + 1).is_some_and(
                |t| {
                    let l = t.to_lowercase();
                    l == "desc"
                        || l == "descending"
                        || l == "highest"
                        || l == "largest"
                },
            );
            orders.push(OrderIntent {
                column: col,
                descending,
            });
        }
    }
    orders
}

fn extract_limit(tokens: &[&str]) -> Option<u64> {
    let limit_keywords = ["top", "first", "limit"];
    for (idx, token) in tokens.iter().enumerate() {
        let lower = token.to_lowercase();
        if !limit_keywords.contains(&lower.as_str()) {
            continue;
        }
        if let Some(next) = tokens.get(idx + 1) {
            if let Ok(n) = next.parse::<u64>() {
                return Some(n);
            }
        }
        if idx > 0 {
            if let Ok(n) = tokens[idx - 1].parse::<u64>() {
                return Some(n);
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::schema::{ColumnInfo, ForeignKey, TableInfo};

    fn test_schema() -> SchemaInfo {
        let mut schema = SchemaInfo::new();
        schema.add_table(TableInfo::new(
            "users",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("name", "TEXT").not_null(),
                ColumnInfo::new("email", "TEXT"),
                ColumnInfo::new("age", "INTEGER"),
            ],
        ));
        let mut orders = TableInfo::new(
            "orders",
            vec![
                ColumnInfo::new("id", "INTEGER").primary_key(),
                ColumnInfo::new("user_id", "INTEGER").not_null(),
                ColumnInfo::new("amount", "REAL").not_null(),
                ColumnInfo::new("status", "TEXT"),
            ],
        );
        orders.add_foreign_key(ForeignKey {
            columns: vec!["user_id".into()],
            referenced_table: "users".into(),
            referenced_columns: vec!["id".into()],
        });
        schema.add_table(orders);
        schema
    }

    #[test]
    fn parse_simple_select() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let intent = parser
            .parse("show all users")
            .expect("test");
        assert!(intent.tables.contains(&"users".to_string()));
    }

    #[test]
    fn parse_with_filter() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let intent = parser
            .parse("find users where age greater than 30")
            .expect("test");
        assert!(!intent.filters.is_empty());
        assert_eq!(intent.filters[0].column, "age");
        assert_eq!(intent.filters[0].op, FilterOp::Gt);
        assert_eq!(intent.filters[0].value, "30");
    }

    #[test]
    fn parse_aggregate() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let intent = parser
            .parse("count of users")
            .expect("test");
        assert!(!intent.aggregates.is_empty());
        assert_eq!(intent.aggregates[0].function, "count");
    }

    #[test]
    fn parse_with_limit() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let intent = parser
            .parse("show top 10 users")
            .expect("test");
        assert_eq!(intent.limit, Some(10));
    }

    #[test]
    fn parse_with_order() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let intent = parser
            .parse("show users sorted by age desc")
            .expect("test");
        assert!(!intent.order_by.is_empty());
        assert_eq!(intent.order_by[0].column, "age");
        assert!(intent.order_by[0].descending);
    }

    #[test]
    fn parse_join_detected() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let intent = parser
            .parse("show users and their orders")
            .expect("test");
        assert!(intent.tables.len() >= 2);
        assert!(!intent.joins.is_empty());
    }

    #[test]
    fn parse_no_tables_error() {
        let schema = test_schema();
        let parser = IntentParser::new(&schema);
        let result = parser.parse("hello world");
        assert!(result.is_err());
    }

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(
            normalize("Show me the user's name!"),
            "show me the user s name"
        );
    }

    #[test]
    fn singularize_basic() {
        assert_eq!(singularize("users"), "user");
        assert_eq!(singularize("categories"), "category");
        assert_eq!(singularize("addresses"), "addres");
    }

    #[test]
    fn extract_limit_from_tokens() {
        let tokens = vec!["top", "5", "results"];
        assert_eq!(extract_limit(&tokens), Some(5));
    }
}

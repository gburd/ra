//! SQL Server Full-Text Search extension.
//!
//! SQL Server provides full-text search through CONTAINS, FREETEXT, CONTAINSTABLE,
//! and FREETEXTTABLE predicates and functions.
//!
//! # Key Features
//!
//! ## CONTAINS Predicate
//!
//! CONTAINS supports precise searches with boolean operators:
//!
//! ```sql
//! -- Simple term search
//! SELECT * FROM articles WHERE CONTAINS(body, 'database');
//!
//! -- Multiple terms with AND
//! SELECT * FROM articles WHERE CONTAINS(body, 'database AND performance');
//!
//! -- Multiple terms with OR
//! SELECT * FROM articles WHERE CONTAINS(body, 'mysql OR postgresql');
//!
//! -- Phrase search
//! SELECT * FROM articles WHERE CONTAINS(body, '"query optimization"');
//!
//! -- Prefix search (wildcard)
//! SELECT * FROM articles WHERE CONTAINS(body, '"optim*"');
//! ```
//!
//! ## NEAR Operator
//!
//! Searches for terms near each other:
//!
//! ```sql
//! -- Terms within default distance (50 terms)
//! SELECT * FROM articles WHERE CONTAINS(body, 'NEAR((database, performance))');
//!
//! -- Terms within specific distance (5 terms)
//! SELECT * FROM articles WHERE CONTAINS(body, 'NEAR((database, performance), 5)');
//!
//! -- Ordered proximity (database must precede performance)
//! SELECT * FROM articles WHERE CONTAINS(body, 'NEAR((database, performance), 5, TRUE)');
//! ```
//!
//! ## ISABOUT Operator
//!
//! Weighted search for relevance ranking:
//!
//! ```sql
//! SELECT * FROM articles
//! WHERE CONTAINS(body, 'ISABOUT(database WEIGHT(0.8), performance WEIGHT(0.5))');
//! ```
//!
//! ## FORMSOF Operator
//!
//! Searches for inflectional or thesaurus forms:
//!
//! ```sql
//! -- Inflectional forms (run, runs, running, ran)
//! SELECT * FROM articles
//! WHERE CONTAINS(body, 'FORMSOF(INFLECTIONAL, run)');
//!
//! -- Thesaurus forms (using configured thesaurus)
//! SELECT * FROM articles
//! WHERE CONTAINS(body, 'FORMSOF(THESAURUS, database)');
//! ```
//!
//! ## FREETEXT Predicate
//!
//! Natural language search (less precise but more flexible):
//!
//! ```sql
//! SELECT * FROM articles WHERE FREETEXT(body, 'database performance optimization');
//! ```
//!
//! ## CONTAINSTABLE and FREETEXTTABLE Functions
//!
//! Table-valued functions that return relevance ranking:
//!
//! ```sql
//! -- CONTAINSTABLE with rank
//! SELECT a.title, ct.RANK
//! FROM articles a
//! INNER JOIN CONTAINSTABLE(articles, body, 'database') AS ct
//!   ON a.id = ct.[KEY]
//! ORDER BY ct.RANK DESC;
//!
//! -- FREETEXTTABLE with rank
//! SELECT a.title, ft.RANK
//! FROM articles a
//! INNER JOIN FREETEXTTABLE(articles, body, 'database performance') AS ft
//!   ON a.id = ft.[KEY]
//! ORDER BY ft.RANK DESC;
//! ```

use sqlparser::ast::Statement;
use std::error::Error;

use crate::grammar::extension::GrammarExtension;

/// SQL Server Full-Text Search extension.
pub struct SQLServerFTSExtension;

impl GrammarExtension for SQLServerFTSExtension {
    fn name(&self) -> &str {
        "sqlserver_fts"
    }

    fn keywords(&self) -> Vec<&str> {
        vec![
            "CONTAINS",
            "FREETEXT",
            "CONTAINSTABLE",
            "FREETEXTTABLE",
            "NEAR",
            "ISABOUT",
            "WEIGHT",
            "FORMSOF",
            "INFLECTIONAL",
            "THESAURUS",
        ]
    }

    fn operators(&self) -> Vec<&str> {
        vec![
            "AND", "OR", "AND NOT",  // Boolean operators in CONTAINS
        ]
    }

    fn functions(&self) -> Vec<&str> {
        vec![
            "CONTAINS",
            "FREETEXT",
            "CONTAINSTABLE",
            "FREETEXTTABLE",
        ]
    }

    fn parse_statement(&self, _sql: &str) -> Result<Option<Statement>, Box<dyn Error>> {
        Ok(None)
    }

    fn documentation_url(&self) -> Option<&str> {
        Some("https://learn.microsoft.com/en-us/sql/relational-databases/search/full-text-search")
    }

    fn min_version(&self) -> Option<&str> {
        Some("2017")
    }
}

/// SQL Server full-text search predicate type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SQLServerFTSType {
    /// CONTAINS predicate (precise search).
    Contains,
    /// FREETEXT predicate (natural language).
    FreeText,
    /// CONTAINSTABLE function (precise with ranking).
    ContainsTable,
    /// FREETEXTTABLE function (natural language with ranking).
    FreeTextTable,
}

/// Parsed CONTAINS expression.
#[derive(Debug, Clone, PartialEq)]
pub struct SQLServerContainsExpr {
    /// Column(s) to search in.
    pub columns: Vec<String>,
    /// Search query.
    pub query: ContainsQuery,
}

/// CONTAINS query element.
#[derive(Debug, Clone, PartialEq)]
pub enum ContainsQuery {
    /// Simple term.
    Term(String),
    /// Phrase (quoted).
    Phrase(String),
    /// Prefix search (wildcard).
    Prefix(String),
    /// AND conjunction.
    And(Box<ContainsQuery>, Box<ContainsQuery>),
    /// OR disjunction.
    Or(Box<ContainsQuery>, Box<ContainsQuery>),
    /// AND NOT negation.
    AndNot(Box<ContainsQuery>, Box<ContainsQuery>),
    /// NEAR proximity search.
    Near {
        /// Terms to search for in proximity.
        terms: Vec<String>,
        /// Maximum distance between terms.
        distance: Option<u32>,
        /// Whether terms must appear in order.
        ordered: bool,
    },
    /// ISABOUT weighted search.
    IsAbout(Vec<WeightedTerm>),
    /// FORMSOF inflectional or thesaurus.
    FormsOf {
        /// Type of forms to search for.
        form_type: FormsOfType,
        /// Terms to find forms of.
        terms: Vec<String>,
    },
}

/// Weighted term for ISABOUT.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedTerm {
    /// The search term.
    pub term: String,
    /// The weight/importance (0.0 to 1.0).
    pub weight: f64,
}

/// Type of forms for FORMSOF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormsOfType {
    /// Inflectional forms (run, runs, running, ran).
    Inflectional,
    /// Thesaurus forms (using configured thesaurus).
    Thesaurus,
}

/// Parse SQL Server CONTAINS query string.
///
/// # Examples
///
/// ```ignore
/// let query = parse_contains_query("database AND performance");
/// // Returns: And(Term("database"), Term("performance"))
/// ```
pub fn parse_contains_query(query: &str) -> Result<ContainsQuery, String> {
    let query = query.trim();

    if query.is_empty() {
        return Err("Empty query".to_string());
    }

    parse_or_expr(query)
}

fn parse_or_expr(query: &str) -> Result<ContainsQuery, String> {
    let parts = split_by_operator_case_insensitive(query, "OR");
    if parts.len() > 1 {
        let left = parse_and_expr(&parts[0])?;
        let right = parse_or_expr(&parts[1..].join(" OR "))?;
        return Ok(ContainsQuery::Or(Box::new(left), Box::new(right)));
    }
    parse_and_expr(query)
}

fn parse_and_expr(query: &str) -> Result<ContainsQuery, String> {
    // Check for AND NOT (case-insensitive)
    let upper_query = query.to_uppercase();
    if let Some(pos) = upper_query.find(" AND NOT ") {
        let left = parse_and_expr(&query[..pos])?;
        let right = parse_primary(&query[pos + 9..])?;
        return Ok(ContainsQuery::AndNot(Box::new(left), Box::new(right)));
    }

    let parts = split_by_operator_case_insensitive(query, "AND");
    if parts.len() > 1 {
        let left = parse_primary(&parts[0])?;
        let right = parse_and_expr(&parts[1..].join(" AND "))?;
        return Ok(ContainsQuery::And(Box::new(left), Box::new(right)));
    }
    parse_primary(query)
}

fn parse_primary(query: &str) -> Result<ContainsQuery, String> {
    let query = query.trim();

    if query.starts_with("NEAR(") {
        return parse_near(query);
    }

    if query.starts_with("ISABOUT(") {
        return parse_isabout(query);
    }

    if query.starts_with("FORMSOF(") {
        return parse_formsof(query);
    }

    if query.starts_with('"') && query.ends_with('"') {
        let phrase = query[1..query.len() - 1].to_string();
        if phrase.ends_with('*') {
            return Ok(ContainsQuery::Prefix(phrase));
        }
        return Ok(ContainsQuery::Phrase(phrase));
    }

    if query.contains(' ') {
        return Err(format!("Unexpected spaces in term: {query}"));
    }

    Ok(ContainsQuery::Term(query.to_string()))
}

fn parse_near(query: &str) -> Result<ContainsQuery, String> {
    if !query.starts_with("NEAR(") || !query.ends_with(')') {
        return Err("Invalid NEAR syntax".to_string());
    }

    let inner = &query[5..query.len() - 1];

    // Find the closing parenthesis of the terms list
    // Pattern: NEAR((term1, term2, ...), distance, ordered)
    // or:      NEAR((term1, term2, ...))
    let terms_end = if let Some(pos) = inner.find(')') {
        pos
    } else {
        return Err("Missing closing parenthesis for terms list".to_string());
    };

    let mut terms_part = &inner[..terms_end];

    // Remove leading '(' if present
    if terms_part.starts_with('(') {
        terms_part = &terms_part[1..];
    }

    let terms: Vec<String> = terms_part
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if terms.is_empty() {
        return Err("NEAR requires at least one term".to_string());
    }

    let mut distance = None;
    let mut ordered = false;

    // Check if there are parameters after the terms list
    if terms_end + 1 < inner.len() {
        let params_str = &inner[terms_end + 1..].trim_start_matches(',').trim();
        if !params_str.is_empty() {
            let param_parts: Vec<&str> = params_str.split(',').map(|s| s.trim()).collect();

            if !param_parts.is_empty() {
                if let Ok(d) = param_parts[0].parse::<u32>() {
                    distance = Some(d);
                }
            }

            if param_parts.len() > 1 {
                ordered = param_parts[1].eq_ignore_ascii_case("TRUE");
            }
        }
    }

    Ok(ContainsQuery::Near {
        terms,
        distance,
        ordered,
    })
}

fn parse_isabout(query: &str) -> Result<ContainsQuery, String> {
    if !query.starts_with("ISABOUT(") || !query.ends_with(')') {
        return Err("Invalid ISABOUT syntax".to_string());
    }

    let inner = &query[8..query.len() - 1];
    let mut weighted_terms = Vec::new();

    for part in inner.split(',') {
        let part = part.trim();
        if let Some(weight_pos) = part.find(" WEIGHT(") {
            let term = part[..weight_pos].trim().to_string();
            let weight_str = &part[weight_pos + 8..];
            let weight_end = weight_str.find(')').ok_or("Missing closing ')' for WEIGHT")?;
            let weight: f64 = weight_str[..weight_end]
                .parse()
                .map_err(|_| "Invalid weight value")?;

            weighted_terms.push(WeightedTerm { term, weight });
        } else {
            weighted_terms.push(WeightedTerm {
                term: part.to_string(),
                weight: 1.0,
            });
        }
    }

    Ok(ContainsQuery::IsAbout(weighted_terms))
}

fn parse_formsof(query: &str) -> Result<ContainsQuery, String> {
    if !query.starts_with("FORMSOF(") || !query.ends_with(')') {
        return Err("Invalid FORMSOF syntax".to_string());
    }

    let inner = &query[8..query.len() - 1];
    let parts: Vec<&str> = inner.splitn(2, ',').collect();

    if parts.len() < 2 {
        return Err("FORMSOF requires form type and at least one term".to_string());
    }

    let form_type = match parts[0].trim().to_uppercase().as_str() {
        "INFLECTIONAL" => FormsOfType::Inflectional,
        "THESAURUS" => FormsOfType::Thesaurus,
        _ => return Err(format!("Unknown FORMSOF type: {}", parts[0])),
    };

    let terms: Vec<String> = parts[1]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if terms.is_empty() {
        return Err("FORMSOF requires at least one term".to_string());
    }

    Ok(ContainsQuery::FormsOf { form_type, terms })
}

fn split_by_operator_case_insensitive(query: &str, operator: &str) -> Vec<String> {
    let operator_with_spaces = format!(" {} ", operator);
    let upper_query = query.to_uppercase();
    let upper_op = operator_with_spaces.to_uppercase();

    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_quotes = false;

    let chars: Vec<char> = query.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
            i += 1;
            continue;
        }

        if !in_quotes {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
            }

            if depth == 0 && i + upper_op.len() <= chars.len() {
                let slice: String = upper_query.chars().skip(i).take(upper_op.len()).collect();
                if slice == upper_op {
                    parts.push(current.clone());
                    current.clear();
                    i += upper_op.len();
                    continue;
                }
            }
        }

        current.push(ch);
        i += 1;
    }

    if !current.is_empty() {
        parts.push(current);
    }

    if parts.is_empty() {
        vec![query.to_string()]
    } else {
        parts
    }
}

#[allow(dead_code)]
fn split_by_operator(query: &str, operator: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_quotes = false;

    let chars: Vec<char> = query.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
            i += 1;
            continue;
        }

        if !in_quotes {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
            }

            if depth == 0 && i + operator.len() <= chars.len() {
                let slice: String = chars[i..i + operator.len()].iter().collect();
                if slice == operator {
                    parts.push(current.clone());
                    current.clear();
                    i += operator.len();
                    continue;
                }
            }
        }

        current.push(ch);
        i += 1;
    }

    if !current.is_empty() {
        parts.push(current);
    }

    if parts.is_empty() {
        vec![query.to_string()]
    } else {
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlserver_fts_extension() {
        let ext = SQLServerFTSExtension;
        assert_eq!(ext.name(), "sqlserver_fts");

        let keywords = ext.keywords();
        assert!(keywords.contains(&"CONTAINS"));
        assert!(keywords.contains(&"FREETEXT"));
        assert!(keywords.contains(&"NEAR"));
        assert!(keywords.contains(&"ISABOUT"));
    }

    #[test]
    fn test_parse_simple_term() {
        let query = parse_contains_query("database").unwrap();
        assert_eq!(query, ContainsQuery::Term("database".to_string()));
    }

    #[test]
    fn test_parse_phrase() {
        let query = parse_contains_query(r#""query optimization""#).unwrap();
        assert_eq!(query, ContainsQuery::Phrase("query optimization".to_string()));
    }

    #[test]
    fn test_parse_prefix() {
        let query = parse_contains_query(r#""optim*""#).unwrap();
        assert_eq!(query, ContainsQuery::Prefix("optim*".to_string()));
    }

    #[test]
    fn test_parse_and() {
        let query = parse_contains_query("database AND performance").unwrap();
        if let ContainsQuery::And(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("database".to_string()));
            assert_eq!(*right, ContainsQuery::Term("performance".to_string()));
        } else {
            panic!("Expected And query");
        }
    }

    #[test]
    fn test_parse_or() {
        let query = parse_contains_query("mysql OR postgresql").unwrap();
        if let ContainsQuery::Or(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("mysql".to_string()));
            assert_eq!(*right, ContainsQuery::Term("postgresql".to_string()));
        } else {
            panic!("Expected Or query");
        }
    }

    #[test]
    fn test_parse_and_not() {
        let query = parse_contains_query("database AND NOT deprecated").unwrap();
        if let ContainsQuery::AndNot(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("database".to_string()));
            assert_eq!(*right, ContainsQuery::Term("deprecated".to_string()));
        } else {
            panic!("Expected AndNot query");
        }
    }

    #[test]
    fn test_parse_near_simple() {
        let query = parse_contains_query("NEAR((database, performance))").unwrap();
        if let ContainsQuery::Near { terms, distance, ordered } = query {
            assert_eq!(terms, vec!["database".to_string(), "performance".to_string()]);
            assert_eq!(distance, None);
            assert!(!ordered);
        } else {
            panic!("Expected Near query");
        }
    }

    #[test]
    fn test_parse_near_with_distance() {
        let query = parse_contains_query("NEAR((database, performance), 5)").unwrap();
        if let ContainsQuery::Near { terms, distance, ordered } = query {
            assert_eq!(terms, vec!["database".to_string(), "performance".to_string()]);
            assert_eq!(distance, Some(5));
            assert!(!ordered);
        } else {
            panic!("Expected Near query");
        }
    }

    #[test]
    fn test_parse_near_ordered() {
        let query = parse_contains_query("NEAR((database, performance), 5, TRUE)").unwrap();
        if let ContainsQuery::Near { terms, distance, ordered } = query {
            assert_eq!(terms, vec!["database".to_string(), "performance".to_string()]);
            assert_eq!(distance, Some(5));
            assert!(ordered);
        } else {
            panic!("Expected Near query");
        }
    }

    #[test]
    fn test_parse_isabout() {
        let query = parse_contains_query("ISABOUT(database WEIGHT(0.8), performance WEIGHT(0.5))").unwrap();
        if let ContainsQuery::IsAbout(terms) = query {
            assert_eq!(terms.len(), 2);
            assert_eq!(terms[0].term, "database");
            assert!((terms[0].weight - 0.8).abs() < 0.001);
            assert_eq!(terms[1].term, "performance");
            assert!((terms[1].weight - 0.5).abs() < 0.001);
        } else {
            panic!("Expected IsAbout query");
        }
    }

    #[test]
    fn test_parse_formsof_inflectional() {
        let query = parse_contains_query("FORMSOF(INFLECTIONAL, run)").unwrap();
        if let ContainsQuery::FormsOf { form_type, terms } = query {
            assert_eq!(form_type, FormsOfType::Inflectional);
            assert_eq!(terms, vec!["run".to_string()]);
        } else {
            panic!("Expected FormsOf query");
        }
    }

    #[test]
    fn test_parse_formsof_thesaurus() {
        let query = parse_contains_query("FORMSOF(THESAURUS, database)").unwrap();
        if let ContainsQuery::FormsOf { form_type, terms } = query {
            assert_eq!(form_type, FormsOfType::Thesaurus);
            assert_eq!(terms, vec!["database".to_string()]);
        } else {
            panic!("Expected FormsOf query");
        }
    }

    #[test]
    fn test_parse_complex_query() {
        let query = parse_contains_query(r#"database AND "query optimization" OR performance"#).unwrap();
        // This tests operator precedence: AND binds tighter than OR
        if let ContainsQuery::Or(left, right) = query {
            if let ContainsQuery::And(left_left, left_right) = *left {
                assert_eq!(*left_left, ContainsQuery::Term("database".to_string()));
                assert_eq!(*left_right, ContainsQuery::Phrase("query optimization".to_string()));
            } else {
                panic!("Expected And in left side of Or");
            }
            assert_eq!(*right, ContainsQuery::Term("performance".to_string()));
        } else {
            panic!("Expected Or query");
        }
    }

    #[test]
    fn test_parse_empty_query() {
        let result = parse_contains_query("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Empty query"));
    }

    #[test]
    fn test_parse_invalid_near() {
        let result = parse_contains_query("NEAR(");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_isabout() {
        let result = parse_contains_query("ISABOUT(database WEIGHT(invalid))");
        assert!(result.is_err());
    }

    // Additional edge case tests

    #[test]
    fn test_parse_multiple_ands() {
        let query = parse_contains_query("term1 AND term2 AND term3").unwrap();
        // Parses as right-associative: term1 AND (term2 AND term3)
        if let ContainsQuery::And(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("term1".to_string()));
            if let ContainsQuery::And(_, _) = *right {
                // Expected nested And on right
            } else {
                panic!("Expected nested And on right");
            }
        } else {
            panic!("Expected And query");
        }
    }

    #[test]
    fn test_parse_multiple_ors() {
        let query = parse_contains_query("term1 OR term2 OR term3").unwrap();
        if let ContainsQuery::Or(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("term1".to_string()));
            if let ContainsQuery::Or(_, _) = *right {
                // Right side should be term2 OR term3
            } else {
                panic!("Expected nested Or on right");
            }
        } else {
            panic!("Expected Or query");
        }
    }

    #[test]
    fn test_parse_and_or_precedence() {
        // AND has higher precedence than OR
        let query = parse_contains_query("term1 OR term2 AND term3").unwrap();
        if let ContainsQuery::Or(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("term1".to_string()));
            if let ContainsQuery::And(_, _) = *right {
                // Right side should be term2 AND term3
            } else {
                panic!("Expected And on right side of Or");
            }
        } else {
            panic!("Expected Or query");
        }
    }

    #[test]
    fn test_parse_phrase_with_numbers() {
        let query = parse_contains_query(r#""version 2024.1""#).unwrap();
        assert_eq!(query, ContainsQuery::Phrase("version 2024.1".to_string()));
    }

    #[test]
    fn test_parse_phrase_with_special_chars() {
        let query = parse_contains_query(r#""database-optimization-guide""#).unwrap();
        assert_eq!(query, ContainsQuery::Phrase("database-optimization-guide".to_string()));
    }

    #[test]
    fn test_parse_multiple_prefixes() {
        let query = parse_contains_query(r#""data*" AND "performa*""#).unwrap();
        if let ContainsQuery::And(left, right) = query {
            assert_eq!(*left, ContainsQuery::Prefix("data*".to_string()));
            assert_eq!(*right, ContainsQuery::Prefix("performa*".to_string()));
        } else {
            panic!("Expected And query");
        }
    }

    #[test]
    fn test_parse_near_single_term() {
        let result = parse_contains_query("NEAR((database))");
        assert!(result.is_ok());
        if let Ok(ContainsQuery::Near { terms, .. }) = result {
            assert_eq!(terms.len(), 1);
        }
    }

    #[test]
    fn test_parse_near_many_terms() {
        let query = parse_contains_query("NEAR((term1, term2, term3, term4))").unwrap();
        if let ContainsQuery::Near { terms, distance, ordered } = query {
            assert_eq!(terms.len(), 4);
            assert_eq!(distance, None);
            assert!(!ordered);
        } else {
            panic!("Expected Near query");
        }
    }

    #[test]
    fn test_parse_near_zero_distance() {
        let query = parse_contains_query("NEAR((term1, term2), 0)").unwrap();
        if let ContainsQuery::Near { terms: _, distance, ordered } = query {
            assert_eq!(distance, Some(0));
            assert!(!ordered);
        } else {
            panic!("Expected Near query");
        }
    }

    #[test]
    fn test_parse_near_large_distance() {
        let query = parse_contains_query("NEAR((term1, term2), 1000)").unwrap();
        if let ContainsQuery::Near { distance, .. } = query {
            assert_eq!(distance, Some(1000));
        } else {
            panic!("Expected Near query");
        }
    }

    #[test]
    fn test_parse_isabout_single_term() {
        let query = parse_contains_query("ISABOUT(database)").unwrap();
        if let ContainsQuery::IsAbout(terms) = query {
            assert_eq!(terms.len(), 1);
            assert_eq!(terms[0].term, "database");
            assert!((terms[0].weight - 1.0).abs() < 0.001);
        } else {
            panic!("Expected IsAbout query");
        }
    }

    #[test]
    fn test_parse_isabout_multiple_terms_no_weights() {
        let query = parse_contains_query("ISABOUT(database, performance, optimization)").unwrap();
        if let ContainsQuery::IsAbout(terms) = query {
            assert_eq!(terms.len(), 3);
            for term in terms {
                assert!((term.weight - 1.0).abs() < 0.001);
            }
        } else {
            panic!("Expected IsAbout query");
        }
    }

    #[test]
    fn test_parse_isabout_zero_weight() {
        let query = parse_contains_query("ISABOUT(database WEIGHT(0.0))").unwrap();
        if let ContainsQuery::IsAbout(terms) = query {
            assert!((terms[0].weight - 0.0).abs() < 0.001);
        } else {
            panic!("Expected IsAbout query");
        }
    }

    #[test]
    fn test_parse_isabout_weight_greater_than_one() {
        let query = parse_contains_query("ISABOUT(database WEIGHT(1.5))").unwrap();
        if let ContainsQuery::IsAbout(terms) = query {
            assert!((terms[0].weight - 1.5).abs() < 0.001);
        } else {
            panic!("Expected IsAbout query");
        }
    }

    #[test]
    fn test_parse_formsof_multiple_terms() {
        let query = parse_contains_query("FORMSOF(INFLECTIONAL, run, jump, swim)").unwrap();
        if let ContainsQuery::FormsOf { form_type, terms } = query {
            assert_eq!(form_type, FormsOfType::Inflectional);
            assert_eq!(terms.len(), 3);
            assert_eq!(terms[0], "run");
            assert_eq!(terms[1], "jump");
            assert_eq!(terms[2], "swim");
        } else {
            panic!("Expected FormsOf query");
        }
    }

    #[test]
    fn test_parse_formsof_invalid_type() {
        let result = parse_contains_query("FORMSOF(INVALID, term)");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown FORMSOF type"));
    }

    #[test]
    fn test_parse_formsof_no_terms() {
        let result = parse_contains_query("FORMSOF(INFLECTIONAL)");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_complex_boolean_with_near() {
        let query = parse_contains_query("database AND NEAR((performance, optimization), 5)").unwrap();
        if let ContainsQuery::And(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("database".to_string()));
            if let ContainsQuery::Near { .. } = *right {
                // Expected
            } else {
                panic!("Expected Near on right");
            }
        } else {
            panic!("Expected And query");
        }
    }

    #[test]
    fn test_parse_complex_boolean_with_isabout() {
        let query = parse_contains_query("ISABOUT(database WEIGHT(0.9)) OR performance").unwrap();
        if let ContainsQuery::Or(left, right) = query {
            if let ContainsQuery::IsAbout(_) = *left {
                // Expected
            } else {
                panic!("Expected IsAbout on left");
            }
            assert_eq!(*right, ContainsQuery::Term("performance".to_string()));
        } else {
            panic!("Expected Or query");
        }
    }

    #[test]
    fn test_parse_complex_boolean_with_formsof() {
        let query = parse_contains_query("FORMSOF(THESAURUS, database) AND performance").unwrap();
        if let ContainsQuery::And(left, right) = query {
            if let ContainsQuery::FormsOf { .. } = *left {
                // Expected
            } else {
                panic!("Expected FormsOf on left");
            }
            assert_eq!(*right, ContainsQuery::Term("performance".to_string()));
        } else {
            panic!("Expected And query");
        }
    }

    #[test]
    fn test_parse_nested_and_not() {
        let query = parse_contains_query("term1 AND term2 AND NOT term3").unwrap();
        if let ContainsQuery::AndNot(left, right) = query {
            if let ContainsQuery::And(_, _) = *left {
                // Expected nested And on left
            } else {
                panic!("Expected And on left of AndNot");
            }
            assert_eq!(*right, ContainsQuery::Term("term3".to_string()));
        } else {
            panic!("Expected AndNot query");
        }
    }

    #[test]
    fn test_parse_and_not_with_phrase() {
        let query = parse_contains_query(r#"database AND NOT "old version""#).unwrap();
        if let ContainsQuery::AndNot(left, right) = query {
            assert_eq!(*left, ContainsQuery::Term("database".to_string()));
            assert_eq!(*right, ContainsQuery::Phrase("old version".to_string()));
        } else {
            panic!("Expected AndNot query");
        }
    }

    #[test]
    fn test_parse_term_with_underscores() {
        let query = parse_contains_query("my_table_name").unwrap();
        assert_eq!(query, ContainsQuery::Term("my_table_name".to_string()));
    }

    #[test]
    fn test_parse_term_with_numbers() {
        let query = parse_contains_query("version2024").unwrap();
        assert_eq!(query, ContainsQuery::Term("version2024".to_string()));
    }

    #[test]
    fn test_sqlserver_fts_type_enum() {
        assert_eq!(SQLServerFTSType::Contains, SQLServerFTSType::Contains);
        assert_ne!(SQLServerFTSType::Contains, SQLServerFTSType::FreeText);
        assert_ne!(SQLServerFTSType::ContainsTable, SQLServerFTSType::FreeTextTable);
    }

    #[test]
    fn test_contains_expr_structure() {
        let expr = SQLServerContainsExpr {
            columns: vec!["title".to_string(), "body".to_string()],
            query: ContainsQuery::Term("database".to_string()),
        };

        assert_eq!(expr.columns.len(), 2);
        assert_eq!(expr.columns[0], "title");
        assert_eq!(expr.columns[1], "body");
    }

    #[test]
    fn test_weighted_term_structure() {
        let term = WeightedTerm {
            term: "database".to_string(),
            weight: 0.75,
        };

        assert_eq!(term.term, "database");
        assert!((term.weight - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_formsof_type_enum() {
        assert_eq!(FormsOfType::Inflectional, FormsOfType::Inflectional);
        assert_ne!(FormsOfType::Inflectional, FormsOfType::Thesaurus);
    }

    #[test]
    fn test_parse_whitespace_variations() {
        // Without space, "AND" is part of term name
        let result1 = parse_contains_query("term1ANDterm2");
        assert!(result1.is_err() || matches!(result1.unwrap(), ContainsQuery::Term(_)));

        // With spaces, AND is an operator
        let query2 = parse_contains_query("term1  AND  term2").unwrap();
        let query3 = parse_contains_query("term1 AND term2").unwrap();

        assert!(matches!(query2, ContainsQuery::And(_, _)));
        assert!(matches!(query3, ContainsQuery::And(_, _)));
    }

    #[test]
    fn test_parse_case_insensitive_operators() {
        let query1 = parse_contains_query("term1 and term2").unwrap();
        let query2 = parse_contains_query("term1 AND term2").unwrap();
        let query3 = parse_contains_query("term1 AnD term2").unwrap();

        // All should parse successfully
        assert!(matches!(query1, ContainsQuery::And(_, _)));
        assert!(matches!(query2, ContainsQuery::And(_, _)));
        assert!(matches!(query3, ContainsQuery::And(_, _)));
    }

    #[test]
    fn test_parse_empty_phrase() {
        let result = parse_contains_query(r#""""#);
        assert!(result.is_ok());
        if let Ok(ContainsQuery::Phrase(s)) = result {
            assert_eq!(s, "");
        }
    }
}

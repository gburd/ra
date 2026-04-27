//! Dialect inference using probabilistic feature detection.
//!
//! This module implements automatic SQL dialect detection by analyzing
//! syntactic features, keywords, functions, and operators in the query text.

use std::collections::HashMap;

/// Dialect inference engine using Bayesian probability scoring.
///
/// Analyzes SQL text to detect which dialect and version is most likely
/// being used based on distinctive features.
pub struct DialectInference {
    /// Feature scores for each dialect
    scores: HashMap<String, f64>,
}

impl DialectInference {
    /// Create a new inference engine.
    pub fn new() -> Self {
        Self {
            scores: HashMap::new(),
        }
    }

    /// Detect dialect from SQL tokens.
    ///
    /// Looks for dialect-specific tokens like:
    /// - PostgreSQL: `$1`, `::`, `$$`
    /// - MySQL: backticks, `LIMIT x, y`
    /// - Oracle: `(+)`, `DUAL`
    /// - SQL Server: `[brackets]`, `TOP`
    pub fn detect_from_tokens(&mut self, sql: &str) {
        // PostgreSQL indicators
        if sql.contains("$1") || sql.contains("$2") {
            *self.scores.entry("postgresql".to_string()).or_insert(0.0) += 0.9;
        }
        if sql.contains("::") {
            *self.scores.entry("postgresql".to_string()).or_insert(0.0) += 0.8;
        }
        if sql.contains("$$") {
            *self.scores.entry("postgresql".to_string()).or_insert(0.0) += 0.95;
        }

        // MySQL indicators
        if sql.contains('`') {
            *self.scores.entry("mysql".to_string()).or_insert(0.0) += 0.9;
        }
        if sql.contains("LIMIT") && sql.contains(',') {
            *self.scores.entry("mysql".to_string()).or_insert(0.0) += 0.7;
        }

        // Oracle indicators
        if sql.contains("(+)") {
            *self.scores.entry("oracle".to_string()).or_insert(0.0) += 0.95;
        }
        if sql.to_uppercase().contains(" DUAL") {
            *self.scores.entry("oracle".to_string()).or_insert(0.0) += 0.9;
        }

        // SQL Server indicators: bracketed identifiers like [column_name]
        // Exclude ARRAY[...] syntax which is PostgreSQL
        let has_bracket_identifiers =
            sql.contains('[') && sql.contains(']') && !sql.to_uppercase().contains("ARRAY[");
        if has_bracket_identifiers {
            *self.scores.entry("sqlserver".to_string()).or_insert(0.0) += 0.8;
        }
        if sql.to_uppercase().contains("TOP ") {
            *self.scores.entry("sqlserver".to_string()).or_insert(0.0) += 0.7;
        }
    }

    /// Detect dialect from SQL syntax patterns.
    pub fn detect_from_syntax(&mut self, sql: &str) {
        let upper = sql.to_uppercase();

        // ARRAY syntax (PostgreSQL)
        if upper.contains("ARRAY[") {
            *self.scores.entry("postgresql".to_string()).or_insert(0.0) += 0.9;
        }

        // RETURNING clause (PostgreSQL, some others)
        if upper.contains("RETURNING") {
            *self.scores.entry("postgresql".to_string()).or_insert(0.0) += 0.6;
        }

        // CONNECT BY (Oracle)
        if upper.contains("CONNECT BY") {
            *self.scores.entry("oracle".to_string()).or_insert(0.0) += 0.95;
        }
    }

    /// Detect dialect from function names.
    ///
    /// Certain functions are unique to specific databases:
    /// - PostgreSQL: `string_agg`, `array_agg`, `jsonb_*`
    /// - MySQL: `GROUP_CONCAT`, `DATE_ADD`
    /// - Oracle: `NVL`, `DECODE`
    /// - SQL Server: `ISNULL`, `GETDATE`
    pub fn detect_from_functions(&mut self, sql: &str) {
        let upper = sql.to_uppercase();

        // PostgreSQL functions
        if upper.contains("STRING_AGG") || upper.contains("ARRAY_AGG") {
            *self.scores.entry("postgresql".to_string()).or_insert(0.0) += 0.9;
        }
        if upper.contains("JSONB_") || upper.contains("JSON_") {
            *self.scores.entry("postgresql".to_string()).or_insert(0.0) += 0.7;
        }

        // MySQL functions
        if upper.contains("GROUP_CONCAT") {
            *self.scores.entry("mysql".to_string()).or_insert(0.0) += 0.95;
        }

        // Oracle functions
        if upper.contains("NVL") || upper.contains("DECODE") {
            *self.scores.entry("oracle".to_string()).or_insert(0.0) += 0.9;
        }

        // SQL Server functions
        if upper.contains("ISNULL") || upper.contains("GETDATE") {
            *self.scores.entry("sqlserver".to_string()).or_insert(0.0) += 0.8;
        }
    }

    /// Compute final scores and return the most likely dialect with confidence.
    ///
    /// Returns (dialect, confidence_score) where confidence is 0.0-1.0.
    pub fn compute_scores(&self) -> (String, f64) {
        if self.scores.is_empty() {
            return ("universal".to_string(), 0.5);
        }

        let (dialect, &score) = self
            .scores
            .iter()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .unwrap();

        let total: f64 = self.scores.values().sum();
        let confidence = if total > 0.0 { score / total } else { 0.0 };

        (dialect.clone(), confidence.min(1.0))
    }
}

impl Default for DialectInference {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgresql_detection() {
        let mut inference = DialectInference::new();
        let sql = "SELECT $1::int, ARRAY[1,2,3]";

        inference.detect_from_tokens(sql);
        inference.detect_from_syntax(sql);

        let (dialect, confidence) = inference.compute_scores();
        assert_eq!(dialect, "postgresql");
        assert!(confidence > 0.8);
    }

    #[test]
    fn test_mysql_detection() {
        let mut inference = DialectInference::new();
        let sql = "SELECT `id`, GROUP_CONCAT(name) FROM users LIMIT 10, 20";

        inference.detect_from_tokens(sql);
        inference.detect_from_functions(sql);

        let (dialect, confidence) = inference.compute_scores();
        assert_eq!(dialect, "mysql");
        assert!(confidence > 0.7);
    }

    #[test]
    fn test_oracle_detection() {
        let mut inference = DialectInference::new();
        let sql = "SELECT NVL(name, 'Unknown') FROM users a, orders b WHERE a.id = b.user_id(+)";

        inference.detect_from_tokens(sql);
        inference.detect_from_functions(sql);

        let (dialect, confidence) = inference.compute_scores();
        assert_eq!(dialect, "oracle");
        assert!(confidence > 0.8);
    }

    #[test]
    fn test_ambiguous_query() {
        let mut inference = DialectInference::new();
        let sql = "SELECT id, name FROM users WHERE active = true";

        inference.detect_from_tokens(sql);
        inference.detect_from_syntax(sql);
        inference.detect_from_functions(sql);

        let (dialect, confidence) = inference.compute_scores();
        assert_eq!(dialect, "universal");
        assert!(confidence <= 0.6); // Low confidence for ambiguous queries
    }
}

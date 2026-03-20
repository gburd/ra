//! Workload analysis and query representation

use anyhow::{Context, Result};
use ra_core::LogicalPlan;
use ra_parser::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Unique identifier for a query
pub type QueryId = String;

/// A query in the workload with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    /// Unique identifier
    pub id: QueryId,
    /// Original SQL text
    pub sql: String,
    /// Parsed logical plan
    #[serde(skip)]
    pub parsed_query: LogicalPlan,
    /// Execution frequency (times per day)
    pub frequency: u32,
    /// Average execution time in milliseconds (if known)
    pub avg_duration_ms: Option<f64>,
}

/// A workload consisting of multiple queries
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Workload {
    /// All queries in the workload
    pub queries: Vec<Query>,
    /// Optional metadata about the workload
    pub metadata: WorkloadMetadata,
}

/// Metadata about a workload
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WorkloadMetadata {
    /// Source of the workload (e.g., "production", "benchmark")
    pub source: Option<String>,
    /// Time period covered (e.g., "2024-03-01 to 2024-03-07")
    pub period: Option<String>,
    /// Total number of query executions
    pub total_executions: Option<u64>,
}

impl Workload {
    /// Create a new empty workload
    pub fn new() -> Self {
        Self::default()
    }

    /// Load workload from a SQL file
    /// Each query should be separated by semicolons
    /// Comments starting with -- frequency: N specify execution frequency
    pub fn from_sql_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read workload file: {:?}", path.as_ref()))?;

        Self::from_sql_string(&content)
    }

    /// Parse workload from SQL string
    pub fn from_sql_string(sql: &str) -> Result<Self> {
        let mut workload = Workload::new();
        let parser = Parser::new();

        // Split by semicolons and parse each query
        let queries: Vec<&str> = sql
            .split(';')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && !s.starts_with("--"))
            .collect();

        for (i, query_sql) in queries.iter().enumerate() {
            // Look for frequency comment
            let frequency = Self::extract_frequency(query_sql);

            // Parse the query
            let parsed = parser.parse(query_sql)
                .with_context(|| format!("Failed to parse query {}: {}", i + 1, query_sql))?;

            let query = Query {
                id: format!("q{}", i + 1),
                sql: query_sql.to_string(),
                parsed_query: parsed,
                frequency: frequency.unwrap_or(1),
                avg_duration_ms: None,
            };

            workload.queries.push(query);
        }

        Ok(workload)
    }

    /// Extract frequency from comment in SQL
    fn extract_frequency(sql: &str) -> Option<u32> {
        for line in sql.lines() {
            if line.trim_start().starts_with("-- frequency:") {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 {
                    return parts[1].trim().parse().ok();
                }
            }
        }
        None
    }

    /// Load workload from PostgreSQL slow query log
    pub fn from_postgres_log(path: impl AsRef<Path>, min_duration_ms: f64) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read log file: {:?}", path.as_ref()))?;

        let mut workload = Workload::new();
        let parser = Parser::new();

        // Simple PostgreSQL log parsing (would be more sophisticated in production)
        let mut current_query = String::new();
        let mut current_duration: Option<f64> = None;
        let mut query_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

        for line in content.lines() {
            if line.contains("duration:") && line.contains("ms") {
                // Extract duration
                if let Some(duration_str) = line.split("duration:").nth(1) {
                    if let Some(ms_pos) = duration_str.find("ms") {
                        if let Ok(duration) = duration_str[..ms_pos].trim().parse::<f64>() {
                            if duration >= min_duration_ms {
                                current_duration = Some(duration);
                            }
                        }
                    }
                }
            } else if line.contains("statement:") || line.contains("execute") {
                // Extract query
                if let Some(query_start) = line.find("statement:").or_else(|| line.find("execute")) {
                    let start_pos = line[query_start..].find(':').unwrap_or(0) + query_start + 1;
                    current_query = line[start_pos..].trim().to_string();
                }
            }

            // If we have both query and duration, add to workload
            if !current_query.is_empty() && current_duration.is_some() {
                *query_counts.entry(current_query.clone()).or_insert(0) += 1;
                current_query.clear();
                current_duration = None;
            }
        }

        // Convert to Query objects
        for (i, (sql, count)) in query_counts.iter().enumerate() {
            if let Ok(parsed) = parser.parse(sql) {
                let query = Query {
                    id: format!("q{}", i + 1),
                    sql: sql.clone(),
                    parsed_query: parsed,
                    frequency: *count,
                    avg_duration_ms: None,
                };
                workload.queries.push(query);
            }
        }

        Ok(workload)
    }

    /// Add a query to the workload
    pub fn add_query(&mut self, sql: impl Into<String>, frequency: u32) -> Result<()> {
        let parser = Parser::new();
        let sql = sql.into();
        let parsed = parser.parse(&sql)?;

        let query = Query {
            id: format!("q{}", self.queries.len() + 1),
            sql,
            parsed_query: parsed,
            frequency,
            avg_duration_ms: None,
        };

        self.queries.push(query);
        Ok(())
    }

    /// Get total number of query executions per day
    pub fn total_executions(&self) -> u64 {
        self.queries.iter().map(|q| q.frequency as u64).sum()
    }

    /// Sort queries by frequency (highest first)
    pub fn sort_by_frequency(&mut self) {
        self.queries.sort_by_key(|q| std::cmp::Reverse(q.frequency));
    }

    /// Get the most frequent queries
    pub fn top_queries(&self, n: usize) -> Vec<&Query> {
        let mut sorted = self.queries.iter().collect::<Vec<_>>();
        sorted.sort_by_key(|q| std::cmp::Reverse(q.frequency));
        sorted.into_iter().take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workload() {
        let sql = r#"
            -- frequency: 1000
            SELECT * FROM users WHERE email = 'test@example.com';

            -- frequency: 500
            SELECT * FROM orders WHERE user_id = 123;

            SELECT * FROM products WHERE category = 'electronics';
        "#;

        let workload = Workload::from_sql_string(sql).unwrap();
        assert_eq!(workload.queries.len(), 3);
        assert_eq!(workload.queries[0].frequency, 1000);
        assert_eq!(workload.queries[1].frequency, 500);
        assert_eq!(workload.queries[2].frequency, 1); // Default frequency
    }

    #[test]
    fn test_total_executions() {
        let mut workload = Workload::new();
        workload.add_query("SELECT 1", 100).unwrap();
        workload.add_query("SELECT 2", 200).unwrap();
        workload.add_query("SELECT 3", 50).unwrap();

        assert_eq!(workload.total_executions(), 350);
    }

    #[test]
    fn test_top_queries() {
        let mut workload = Workload::new();
        workload.add_query("SELECT 1", 10).unwrap();
        workload.add_query("SELECT 2", 100).unwrap();
        workload.add_query("SELECT 3", 50).unwrap();

        let top = workload.top_queries(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].frequency, 100);
        assert_eq!(top[1].frequency, 50);
    }
}
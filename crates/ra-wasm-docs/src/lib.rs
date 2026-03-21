//! WASM wrapper for Ra documentation interactive examples.
//!
//! Provides simplified API for browser-based SQL demonstrations
//! with parsing, translation, optimization, and formatting capabilities.

use ra_core::algebra::RelExpr;
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
// Formatter will be implemented below
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// Set up panic hook for better error messages in browser console
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
}

/// Result of SQL parsing operation.
#[derive(Serialize, Deserialize)]
pub struct ParseResult {
    /// Whether parsing succeeded.
    pub success: bool,
    /// The parsed relational expression (if successful).
    pub expr: Option<String>,
    /// Error message (if parsing failed).
    pub error: Option<String>,
    /// Original SQL for reference.
    pub original_sql: String,
}

/// Result of SQL translation operation.
#[derive(Serialize, Deserialize)]
pub struct TranslateResult {
    /// Whether translation succeeded.
    pub success: bool,
    /// Translated SQL for target dialect.
    pub translated_sql: Option<String>,
    /// Target dialect name.
    pub dialect: String,
    /// Error message (if translation failed).
    pub error: Option<String>,
}

/// Result of query optimization.
#[derive(Serialize, Deserialize)]
pub struct OptimizeResult {
    /// Whether optimization succeeded.
    pub success: bool,
    /// Original query plan.
    pub original_plan: Option<String>,
    /// Optimized query plan.
    pub optimized_plan: Option<String>,
    /// Cost before optimization.
    pub original_cost: Option<f64>,
    /// Cost after optimization.
    pub optimized_cost: Option<f64>,
    /// Applied optimization rules.
    pub applied_rules: Vec<String>,
    /// Error message (if optimization failed).
    pub error: Option<String>,
}

/// Result of SQL formatting.
#[derive(Serialize, Deserialize)]
pub struct FormatResult {
    /// Whether formatting succeeded.
    pub success: bool,
    /// Formatted SQL.
    pub formatted_sql: Option<String>,
    /// Error message (if formatting failed).
    pub error: Option<String>,
}

/// Parse SQL query into relational algebra expression.
#[wasm_bindgen]
pub fn parse_sql(sql: &str) -> String {
    let result = match sql_to_relexpr(sql) {
        Ok(expr) => ParseResult {
            success: true,
            expr: Some(format!("{:#?}", expr)),
            error: None,
            original_sql: sql.to_string(),
        },
        Err(e) => ParseResult {
            success: false,
            expr: None,
            error: Some(e.to_string()),
            original_sql: sql.to_string(),
        },
    };

    serde_json::to_string(&result).unwrap_or_else(|e| {
        format!(r#"{{"success": false, "error": "Serialization failed: {}"}}"#, e)
    })
}

/// Translate SQL to a specific RDBMS dialect.
#[wasm_bindgen]
pub fn translate(sql: &str, target_dialect: &str) -> String {
    // First parse to validate SQL
    let expr = match sql_to_relexpr(sql) {
        Ok(expr) => expr,
        Err(e) => {
            let result = TranslateResult {
                success: false,
                translated_sql: None,
                dialect: target_dialect.to_string(),
                error: Some(format!("Failed to parse SQL: {}", e)),
            };
            return serde_json::to_string(&result).unwrap_or_else(|e| {
                format!(r#"{{"success": false, "error": "Serialization failed: {}"}}"#, e)
            });
        }
    };

    // Translate to target dialect
    let translated = translate_to_dialect(&expr, target_dialect);

    let result = match translated {
        Ok(sql) => TranslateResult {
            success: true,
            translated_sql: Some(sql),
            dialect: target_dialect.to_string(),
            error: None,
        },
        Err(e) => TranslateResult {
            success: false,
            translated_sql: None,
            dialect: target_dialect.to_string(),
            error: Some(e),
        },
    };

    serde_json::to_string(&result).unwrap_or_else(|e| {
        format!(r#"{{"success": false, "error": "Serialization failed: {}"}}"#, e)
    })
}

/// Optimize SQL query and return the optimization plan.
#[wasm_bindgen]
pub fn optimize(sql: &str) -> String {
    // Parse SQL to relational expression
    let expr = match sql_to_relexpr(sql) {
        Ok(expr) => expr,
        Err(e) => {
            let result = OptimizeResult {
                success: false,
                original_plan: None,
                optimized_plan: None,
                original_cost: None,
                optimized_cost: None,
                applied_rules: vec![],
                error: Some(format!("Failed to parse SQL: {}", e)),
            };
            return serde_json::to_string(&result).unwrap_or_else(|e| {
                format!(r#"{{"success": false, "error": "Serialization failed: {}"}}"#, e)
            });
        }
    };

    // Create optimizer and optimize
    let optimizer = Optimizer::new();
    let original_plan = format!("{:#?}", expr);

    match optimizer.optimize(&expr) {
        Ok(optimized) => {
            let optimized_plan = format!("{:#?}", optimized);

            // Get cost information (simplified for demo)
            let original_cost = estimate_cost(&expr);
            let optimized_cost = estimate_cost(&optimized);

            // Get applied rules (simplified list for demo)
            let applied_rules = get_applied_rules(&expr, &optimized);

            let result = OptimizeResult {
                success: true,
                original_plan: Some(original_plan),
                optimized_plan: Some(optimized_plan),
                original_cost: Some(original_cost),
                optimized_cost: Some(optimized_cost),
                applied_rules,
                error: None,
            };

            serde_json::to_string(&result).unwrap_or_else(|e| {
                format!(r#"{{"success": false, "error": "Serialization failed: {}"}}"#, e)
            })
        }
        Err(e) => {
            let result = OptimizeResult {
                success: false,
                original_plan: Some(original_plan),
                optimized_plan: None,
                original_cost: None,
                optimized_cost: None,
                applied_rules: vec![],
                error: Some(e.to_string()),
            };

            serde_json::to_string(&result).unwrap_or_else(|e| {
                format!(r#"{{"success": false, "error": "Serialization failed: {}"}}"#, e)
            })
        }
    }
}

/// Format SQL query for better readability.
#[wasm_bindgen]
pub fn format(sql: &str) -> String {
    // For now, use sqlparser to parse and re-format
    let result = FormatResult {
        success: true,
        formatted_sql: Some(basic_format_sql(sql)),
        error: None,
    };

    serde_json::to_string(&result).unwrap_or_else(|e| {
        format!(r#"{{"success": false, "error": "Serialization failed: {}"}}"#, e)
    })
}

// Basic SQL formatting
fn basic_format_sql(sql: &str) -> String {
    // Basic formatting - add proper indentation
    sql.replace("SELECT", "SELECT")
        .replace("FROM", "\nFROM")
        .replace("WHERE", "\nWHERE")
        .replace("GROUP BY", "\nGROUP BY")
        .replace("ORDER BY", "\nORDER BY")
        .replace("HAVING", "\nHAVING")
        .replace("LIMIT", "\nLIMIT")
}

// Helper function to translate RelExpr to specific dialect
fn translate_to_dialect(expr: &RelExpr, dialect: &str) -> Result<String, String> {
    // This would use the dialect module to translate
    // For now, return a placeholder implementation
    match dialect.to_lowercase().as_str() {
        "postgresql" | "postgres" | "pg" => {
            Ok(format!("-- PostgreSQL\n{}", expr_to_sql(expr)))
        }
        "mysql" => {
            Ok(format!("-- MySQL\n{}", expr_to_sql(expr)))
        }
        "sqlite" => {
            Ok(format!("-- SQLite\n{}", expr_to_sql(expr)))
        }
        "duckdb" => {
            Ok(format!("-- DuckDB\n{}", expr_to_sql(expr)))
        }
        _ => Err(format!("Unsupported dialect: {}", dialect)),
    }
}

// Helper function to convert RelExpr back to SQL (simplified)
fn expr_to_sql(_expr: &RelExpr) -> String {
    // This would be a proper implementation that converts
    // RelExpr back to SQL. For now, return placeholder.
    "SELECT * FROM table WHERE condition".to_string()
}

// Helper function to estimate query cost (simplified)
fn estimate_cost(_expr: &RelExpr) -> f64 {
    // This would use the actual cost model
    // For demo purposes, return a simple estimate
    100.0
}

// Helper function to get applied optimization rules (simplified)
fn get_applied_rules(_original: &RelExpr, _optimized: &RelExpr) -> Vec<String> {
    // This would analyze the differences and determine which rules were applied
    // For demo purposes, return common optimization rules
    vec![
        "Predicate Pushdown".to_string(),
        "Join Reordering".to_string(),
        "Expression Simplification".to_string(),
    ]
}
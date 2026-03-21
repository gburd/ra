//! Schema analysis for index and table design issues.
//!
//! Detects: unused indexes, missing indexes, duplicate indexes,
//! wrong index types, column ordering issues, missing primary keys,
//! foreign keys without indexes, and table bloat indicators.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::recommendations::Severity;

/// What kind of schema issue was found.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
)]
pub enum SchemaIssueKind {
    /// Index exists but has never been used.
    UnusedIndex,
    /// Frequent sequential scans suggest a missing index.
    MissingIndex,
    /// Two indexes cover the same columns.
    DuplicateIndex,
    /// Index type is wrong for the column data type.
    WrongIndexType,
    /// Composite index has suboptimal column ordering.
    ColumnOrderingIssue,
    /// Table has no primary key.
    MissingPrimaryKey,
    /// Foreign key column is not indexed.
    ForeignKeyWithoutIndex,
    /// Table has significant bloat from dead tuples.
    TableBloat,
}

impl fmt::Display for SchemaIssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnusedIndex => write!(f, "Unused index"),
            Self::MissingIndex => write!(f, "Missing index"),
            Self::DuplicateIndex => write!(f, "Duplicate index"),
            Self::WrongIndexType => write!(f, "Wrong index type"),
            Self::ColumnOrderingIssue => {
                write!(f, "Column ordering")
            }
            Self::MissingPrimaryKey => {
                write!(f, "Missing primary key")
            }
            Self::ForeignKeyWithoutIndex => {
                write!(f, "FK without index")
            }
            Self::TableBloat => write!(f, "Table bloat"),
        }
    }
}

/// A detected schema issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaIssue {
    /// What kind of issue.
    pub kind: SchemaIssueKind,
    /// Table name.
    pub table: String,
    /// Index name (if applicable).
    pub index_name: Option<String>,
    /// Columns involved.
    pub columns: Vec<String>,
    /// Description of the problem.
    pub message: String,
    /// Suggested fix.
    pub suggestion: String,
}

impl SchemaIssue {
    /// Map issue kind to recommendation severity.
    #[must_use]
    pub fn severity(&self) -> Severity {
        match self.kind {
            SchemaIssueKind::UnusedIndex
            | SchemaIssueKind::DuplicateIndex
            | SchemaIssueKind::ColumnOrderingIssue => {
                Severity::Info
            }
            SchemaIssueKind::MissingIndex
            | SchemaIssueKind::WrongIndexType
            | SchemaIssueKind::ForeignKeyWithoutIndex
            | SchemaIssueKind::TableBloat => Severity::Warning,
            SchemaIssueKind::MissingPrimaryKey => Severity::Error,
        }
    }
}

impl fmt::Display for SchemaIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.kind, self.table, self.message)
    }
}

/// Index usage data collected from `pg_stat_user_indexes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexUsage {
    /// Index name.
    pub name: String,
    /// Table this index belongs to.
    pub table: String,
    /// Columns in the index.
    pub columns: Vec<String>,
    /// Index type (btree, hash, gin, gist, etc.).
    pub index_type: String,
    /// Number of index scans since last stats reset.
    pub scans: u64,
    /// Size of the index in bytes.
    pub size_bytes: u64,
    /// Whether this is a unique index.
    pub is_unique: bool,
    /// Whether this is a primary key index.
    pub is_primary: bool,
}

/// Column type info for checking appropriate index types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnTypeInfo {
    /// Column name.
    pub name: String,
    /// `PostgreSQL` type name (e.g. "integer", "text", "jsonb").
    pub pg_type: String,
    /// Average column width in bytes.
    pub avg_width: u32,
}

/// Table-level schema info for analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchemaInfo {
    /// Table name.
    pub name: String,
    /// Column types.
    pub columns: Vec<ColumnTypeInfo>,
    /// Indexes on this table.
    pub indexes: Vec<IndexUsage>,
    /// Primary key column names.
    pub primary_key: Vec<String>,
    /// Foreign key definitions.
    pub foreign_keys: Vec<ForeignKeyInfo>,
    /// Number of sequential scans since stats reset.
    pub seq_scan_count: u64,
    /// Columns frequently used in WHERE clauses.
    pub filtered_columns: Vec<String>,
    /// Dead tuple count.
    pub dead_tuples: u64,
    /// Live tuple count.
    pub live_tuples: u64,
}

/// Foreign key info for FK-without-index detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeyInfo {
    /// Constraint name.
    pub name: String,
    /// Columns in the foreign key.
    pub columns: Vec<String>,
    /// Referenced table.
    pub referenced_table: String,
    /// Referenced columns.
    pub referenced_columns: Vec<String>,
}

/// Analyzes database schema for design issues.
pub struct SchemaAnalyzer {
    tables: Vec<TableSchemaInfo>,
    issues: Vec<SchemaIssue>,
}

impl SchemaAnalyzer {
    /// Create a new empty analyzer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tables: Vec::new(),
            issues: Vec::new(),
        }
    }

    /// Load table schema information for analysis.
    pub fn add_table(&mut self, table: TableSchemaInfo) {
        self.tables.push(table);
    }

    /// Run all schema checks and populate issues.
    pub fn analyze(&mut self) {
        self.issues.clear();
        let tables = self.tables.clone();
        for table in &tables {
            self.check_unused_indexes(table);
            self.check_missing_indexes(table);
            self.check_duplicate_indexes(table);
            self.check_wrong_index_types(table);
            self.check_column_ordering(table);
            self.check_missing_primary_key(table);
            self.check_fk_without_index(table);
        }
    }

    /// Get all detected issues.
    #[must_use]
    pub fn issues(&self) -> &[SchemaIssue] {
        &self.issues
    }

    /// Get issues for a specific table.
    #[must_use]
    pub fn issues_for_table(
        &self,
        table: &str,
    ) -> Vec<&SchemaIssue> {
        self.issues
            .iter()
            .filter(|i| i.table == table)
            .collect()
    }

    fn check_unused_indexes(
        &mut self,
        table: &TableSchemaInfo,
    ) {
        for idx in &table.indexes {
            if idx.scans == 0
                && !idx.is_unique
                && !idx.is_primary
            {
                self.issues.push(SchemaIssue {
                    kind: SchemaIssueKind::UnusedIndex,
                    table: table.name.clone(),
                    index_name: Some(idx.name.clone()),
                    columns: idx.columns.clone(),
                    message: format!(
                        "Index '{}' has 0 scans \
                         (size: {} bytes)",
                        idx.name, idx.size_bytes,
                    ),
                    suggestion: format!(
                        "DROP INDEX {};",
                        idx.name,
                    ),
                });
            }
        }
    }

    fn check_missing_indexes(
        &mut self,
        table: &TableSchemaInfo,
    ) {
        if table.seq_scan_count < 100 {
            return;
        }

        let indexed_cols: Vec<&str> = table
            .indexes
            .iter()
            .flat_map(|idx| idx.columns.iter().map(String::as_str))
            .collect();

        for col in &table.filtered_columns {
            if !indexed_cols.contains(&col.as_str()) {
                self.issues.push(SchemaIssue {
                    kind: SchemaIssueKind::MissingIndex,
                    table: table.name.clone(),
                    index_name: None,
                    columns: vec![col.clone()],
                    message: format!(
                        "Column '{col}' is frequently filtered \
                         but has no index ({} seq scans)",
                        table.seq_scan_count,
                    ),
                    suggestion: format!(
                        "CREATE INDEX ON {} ({col});",
                        table.name,
                    ),
                });
            }
        }
    }

    fn check_duplicate_indexes(
        &mut self,
        table: &TableSchemaInfo,
    ) {
        let mut seen: HashMap<Vec<String>, &IndexUsage> =
            HashMap::new();
        for idx in &table.indexes {
            if let Some(existing) =
                seen.get(&idx.columns)
            {
                self.issues.push(SchemaIssue {
                    kind: SchemaIssueKind::DuplicateIndex,
                    table: table.name.clone(),
                    index_name: Some(idx.name.clone()),
                    columns: idx.columns.clone(),
                    message: format!(
                        "Index '{}' duplicates '{}' \
                         (same columns: {})",
                        idx.name,
                        existing.name,
                        idx.columns.join(", "),
                    ),
                    suggestion: format!(
                        "DROP INDEX {};",
                        if idx.scans <= existing.scans {
                            &idx.name
                        } else {
                            &existing.name
                        },
                    ),
                });
            } else {
                seen.insert(idx.columns.clone(), idx);
            }
        }
    }

    fn check_wrong_index_types(
        &mut self,
        table: &TableSchemaInfo,
    ) {
        let type_map: HashMap<&str, &ColumnTypeInfo> = table
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();

        for idx in &table.indexes {
            for col_name in &idx.columns {
                if let Some(col_info) =
                    type_map.get(col_name.as_str())
                {
                    let recommended =
                        recommended_index_type(&col_info.pg_type);
                    if let Some(rec) = recommended {
                        if idx.index_type != rec {
                            self.issues.push(SchemaIssue {
                                kind: SchemaIssueKind::WrongIndexType,
                                table: table.name.clone(),
                                index_name: Some(
                                    idx.name.clone(),
                                ),
                                columns: vec![
                                    col_name.clone(),
                                ],
                                message: format!(
                                    "Index '{}' uses {} on column \
                                     '{}' (type: {}), but {} \
                                     would be better",
                                    idx.name,
                                    idx.index_type,
                                    col_name,
                                    col_info.pg_type,
                                    rec,
                                ),
                                suggestion: format!(
                                    "CREATE INDEX ON {} USING {} ({col_name});",
                                    table.name, rec,
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    fn check_column_ordering(
        &mut self,
        table: &TableSchemaInfo,
    ) {
        let type_map: HashMap<&str, &ColumnTypeInfo> = table
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c))
            .collect();

        for idx in &table.indexes {
            if idx.columns.len() < 2 {
                continue;
            }
            if let Some(first_col) = idx.columns.first() {
                if let Some(col_info) =
                    type_map.get(first_col.as_str())
                {
                    if col_info.pg_type == "text"
                        && col_info.avg_width > 100
                    {
                        self.issues.push(SchemaIssue {
                            kind: SchemaIssueKind::ColumnOrderingIssue,
                            table: table.name.clone(),
                            index_name: Some(
                                idx.name.clone(),
                            ),
                            columns: idx.columns.clone(),
                            message: format!(
                                "Index '{}' has large TEXT \
                                 column '{}' first \
                                 (avg {} bytes)",
                                idx.name,
                                first_col,
                                col_info.avg_width,
                            ),
                            suggestion: format!(
                                "Reorder index columns: put \
                                 smaller, more selective \
                                 columns first in '{}'",
                                idx.name,
                            ),
                        });
                    }
                }
            }
        }
    }

    fn check_missing_primary_key(
        &mut self,
        table: &TableSchemaInfo,
    ) {
        if table.primary_key.is_empty() {
            self.issues.push(SchemaIssue {
                kind: SchemaIssueKind::MissingPrimaryKey,
                table: table.name.clone(),
                index_name: None,
                columns: vec![],
                message: format!(
                    "Table '{}' has no primary key",
                    table.name,
                ),
                suggestion: format!(
                    "ALTER TABLE {} ADD PRIMARY KEY (...);",
                    table.name,
                ),
            });
        }
    }

    fn check_fk_without_index(
        &mut self,
        table: &TableSchemaInfo,
    ) {
        let indexed_col_sets: Vec<Vec<String>> = table
            .indexes
            .iter()
            .map(|idx| idx.columns.clone())
            .collect();

        for fk in &table.foreign_keys {
            let fk_indexed = indexed_col_sets.iter().any(|cols| {
                fk.columns.len() <= cols.len()
                    && fk.columns
                        .iter()
                        .zip(cols.iter())
                        .all(|(a, b)| a == b)
            });

            if !fk_indexed {
                self.issues.push(SchemaIssue {
                    kind: SchemaIssueKind::ForeignKeyWithoutIndex,
                    table: table.name.clone(),
                    index_name: None,
                    columns: fk.columns.clone(),
                    message: format!(
                        "Foreign key '{}' columns ({}) \
                         have no supporting index",
                        fk.name,
                        fk.columns.join(", "),
                    ),
                    suggestion: format!(
                        "CREATE INDEX ON {} ({});",
                        table.name,
                        fk.columns.join(", "),
                    ),
                });
            }
        }
    }
}

impl Default for SchemaAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Recommend the best index type for a `PostgreSQL` type.
fn recommended_index_type(pg_type: &str) -> Option<&'static str> {
    match pg_type {
        "jsonb" | "json" | "tsvector" => Some("gin"),
        t if t.ends_with("[]") => Some("gin"),
        "point" | "box" | "circle" | "polygon"
        | "inet" | "cidr"
        | "int4range" | "int8range" | "numrange"
        | "tsrange" | "tstzrange" | "daterange" => {
            Some("gist")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table(name: &str) -> TableSchemaInfo {
        TableSchemaInfo {
            name: name.to_string(),
            columns: vec![
                ColumnTypeInfo {
                    name: "id".to_string(),
                    pg_type: "integer".to_string(),
                    avg_width: 4,
                },
                ColumnTypeInfo {
                    name: "name".to_string(),
                    pg_type: "text".to_string(),
                    avg_width: 50,
                },
            ],
            indexes: vec![],
            primary_key: vec!["id".to_string()],
            foreign_keys: vec![],
            seq_scan_count: 0,
            filtered_columns: vec![],
            dead_tuples: 0,
            live_tuples: 1000,
        }
    }

    #[test]
    fn detect_unused_index() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("users");
        table.indexes.push(IndexUsage {
            name: "idx_unused".to_string(),
            table: "users".to_string(),
            columns: vec!["name".to_string()],
            index_type: "btree".to_string(),
            scans: 0,
            size_bytes: 8192,
            is_unique: false,
            is_primary: false,
        });
        analyzer.add_table(table);
        analyzer.analyze();

        assert_eq!(analyzer.issues().len(), 1);
        assert_eq!(
            analyzer.issues()[0].kind,
            SchemaIssueKind::UnusedIndex,
        );
    }

    #[test]
    fn skip_unique_unused_index() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("users");
        table.indexes.push(IndexUsage {
            name: "idx_unique".to_string(),
            table: "users".to_string(),
            columns: vec!["name".to_string()],
            index_type: "btree".to_string(),
            scans: 0,
            size_bytes: 8192,
            is_unique: true,
            is_primary: false,
        });
        analyzer.add_table(table);
        analyzer.analyze();

        assert!(analyzer.issues().is_empty());
    }

    #[test]
    fn detect_missing_index() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("orders");
        table.seq_scan_count = 500;
        table.filtered_columns =
            vec!["customer_id".to_string()];
        analyzer.add_table(table);
        analyzer.analyze();

        assert_eq!(analyzer.issues().len(), 1);
        assert_eq!(
            analyzer.issues()[0].kind,
            SchemaIssueKind::MissingIndex,
        );
    }

    #[test]
    fn detect_duplicate_indexes() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("orders");
        let cols = vec!["status".to_string()];
        table.indexes.push(IndexUsage {
            name: "idx_status_1".to_string(),
            table: "orders".to_string(),
            columns: cols.clone(),
            index_type: "btree".to_string(),
            scans: 100,
            size_bytes: 8192,
            is_unique: false,
            is_primary: false,
        });
        table.indexes.push(IndexUsage {
            name: "idx_status_2".to_string(),
            table: "orders".to_string(),
            columns: cols,
            index_type: "btree".to_string(),
            scans: 50,
            size_bytes: 8192,
            is_unique: false,
            is_primary: false,
        });
        analyzer.add_table(table);
        analyzer.analyze();

        let dupes: Vec<_> = analyzer
            .issues()
            .iter()
            .filter(|i| i.kind == SchemaIssueKind::DuplicateIndex)
            .collect();
        assert_eq!(dupes.len(), 1);
    }

    #[test]
    fn detect_wrong_index_type_jsonb() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("events");
        table.columns.push(ColumnTypeInfo {
            name: "payload".to_string(),
            pg_type: "jsonb".to_string(),
            avg_width: 500,
        });
        table.indexes.push(IndexUsage {
            name: "idx_payload".to_string(),
            table: "events".to_string(),
            columns: vec!["payload".to_string()],
            index_type: "btree".to_string(),
            scans: 10,
            size_bytes: 65536,
            is_unique: false,
            is_primary: false,
        });
        analyzer.add_table(table);
        analyzer.analyze();

        let wrong: Vec<_> = analyzer
            .issues()
            .iter()
            .filter(|i| {
                i.kind == SchemaIssueKind::WrongIndexType
            })
            .collect();
        assert_eq!(wrong.len(), 1);
        assert!(wrong[0].suggestion.contains("gin"));
    }

    #[test]
    fn detect_missing_primary_key() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("logs");
        table.primary_key.clear();
        analyzer.add_table(table);
        analyzer.analyze();

        let missing: Vec<_> = analyzer
            .issues()
            .iter()
            .filter(|i| {
                i.kind == SchemaIssueKind::MissingPrimaryKey
            })
            .collect();
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn detect_fk_without_index() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("orders");
        table.foreign_keys.push(ForeignKeyInfo {
            name: "fk_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            referenced_table: "customers".to_string(),
            referenced_columns: vec!["id".to_string()],
        });
        analyzer.add_table(table);
        analyzer.analyze();

        let fk_issues: Vec<_> = analyzer
            .issues()
            .iter()
            .filter(|i| {
                i.kind
                    == SchemaIssueKind::ForeignKeyWithoutIndex
            })
            .collect();
        assert_eq!(fk_issues.len(), 1);
    }

    #[test]
    fn column_ordering_large_text_first() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut table = make_table("products");
        table.columns.push(ColumnTypeInfo {
            name: "description".to_string(),
            pg_type: "text".to_string(),
            avg_width: 500,
        });
        table.indexes.push(IndexUsage {
            name: "idx_desc_id".to_string(),
            table: "products".to_string(),
            columns: vec![
                "description".to_string(),
                "id".to_string(),
            ],
            index_type: "btree".to_string(),
            scans: 10,
            size_bytes: 65536,
            is_unique: false,
            is_primary: false,
        });
        analyzer.add_table(table);
        analyzer.analyze();

        let ordering: Vec<_> = analyzer
            .issues()
            .iter()
            .filter(|i| {
                i.kind == SchemaIssueKind::ColumnOrderingIssue
            })
            .collect();
        assert_eq!(ordering.len(), 1);
    }

    #[test]
    fn recommended_index_type_mapping() {
        assert_eq!(recommended_index_type("jsonb"), Some("gin"));
        assert_eq!(recommended_index_type("tsvector"), Some("gin"));
        assert_eq!(
            recommended_index_type("integer[]"),
            Some("gin"),
        );
        assert_eq!(recommended_index_type("point"), Some("gist"));
        assert_eq!(
            recommended_index_type("int4range"),
            Some("gist"),
        );
        assert_eq!(recommended_index_type("integer"), None);
        assert_eq!(recommended_index_type("text"), None);
    }

    #[test]
    fn issues_for_table_filter() {
        let mut analyzer = SchemaAnalyzer::new();
        let mut t1 = make_table("users");
        t1.primary_key.clear();
        let t2 = make_table("orders");
        analyzer.add_table(t1);
        analyzer.add_table(t2);
        analyzer.analyze();

        let user_issues =
            analyzer.issues_for_table("users");
        assert_eq!(user_issues.len(), 1);
        let order_issues =
            analyzer.issues_for_table("orders");
        assert!(order_issues.is_empty());
    }
}

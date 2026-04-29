//! Trigger analysis for DML cost estimation and cascade detection.
//!
//! Provides tools to analyze the impact of triggers on DML operations:
//! - Estimate the total cost of DML operations including trigger
//!   overhead
//! - Detect cascading trigger chains that could cause performance
//!   problems or infinite loops

use std::collections::HashSet;

use ra_metadata::schema::{SchemaInfo, TriggerEvent, TriggerInfo, TriggerScope, TriggerTiming};

/// Cost multiplier for different trigger types.
const BEFORE_ROW_COST: f64 = 1.5;
const AFTER_ROW_COST: f64 = 1.2;
const BEFORE_STMT_COST: f64 = 0.1;
const AFTER_STMT_COST: f64 = 0.1;

/// Estimated cost of a DML operation including trigger overhead.
#[derive(Debug, Clone)]
pub struct DmlCostEstimate {
    /// Base cost without triggers.
    pub base_cost: f64,
    /// Cost added by triggers.
    pub trigger_cost: f64,
    /// Total cost (base + trigger).
    pub total_cost: f64,
    /// Number of triggers that fire.
    pub trigger_count: usize,
    /// Breakdown of cost per trigger.
    pub trigger_breakdown: Vec<TriggerCostItem>,
}

/// Cost contribution from a single trigger.
#[derive(Debug, Clone)]
pub struct TriggerCostItem {
    /// Trigger name.
    pub trigger_name: String,
    /// When the trigger fires.
    pub timing: TriggerTiming,
    /// Per-row or per-statement scope.
    pub scope: TriggerScope,
    /// Estimated cost contribution.
    pub estimated_cost: f64,
}

/// Complete trigger analysis for a table.
#[derive(Debug, Clone)]
pub struct TriggerAnalysis {
    /// Table being analyzed.
    pub table_name: String,
    /// Cost estimates per DML event.
    pub insert_cost: Option<DmlCostEstimate>,
    /// Cost estimate for UPDATE operations.
    pub update_cost: Option<DmlCostEstimate>,
    /// Cost estimate for DELETE operations.
    pub delete_cost: Option<DmlCostEstimate>,
    /// Cascade warnings.
    pub cascade_warnings: Vec<CascadeWarning>,
}

/// Warning about a cascading trigger chain.
#[derive(Debug, Clone)]
pub struct CascadeWarning {
    /// Severity: "info", "warning", or "error".
    pub severity: CascadeSeverity,
    /// Human-readable warning message.
    pub message: String,
    /// The chain of triggers involved.
    pub trigger_chain: Vec<String>,
}

/// Severity of a cascade warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CascadeSeverity {
    /// Informational: triggers exist but pose no risk.
    Info,
    /// Warning: potential performance concern.
    Warning,
    /// Error: possible infinite loop detected.
    Error,
}

impl std::fmt::Display for CascadeSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// Estimate the total cost of a DML operation on a table,
/// including trigger execution overhead.
///
/// `base_cost` is the base DML cost without triggers.
/// `estimated_rows` is the expected number of rows affected.
#[must_use]
pub fn analyze_dml_cost(
    table_name: &str,
    event: TriggerEvent,
    base_cost: f64,
    estimated_rows: f64,
    schema: &SchemaInfo,
) -> DmlCostEstimate {
    let Some(table) = schema.get_table(table_name) else {
        return DmlCostEstimate {
            base_cost,
            trigger_cost: 0.0,
            total_cost: base_cost,
            trigger_count: 0,
            trigger_breakdown: vec![],
        };
    };

    let triggers: Vec<&TriggerInfo> = table
        .triggers
        .iter()
        .filter(|t| t.event == event && t.enabled)
        .collect();

    let mut trigger_cost = 0.0;
    let mut breakdown = Vec::new();

    for trigger in &triggers {
        let cost = estimate_trigger_cost(trigger, base_cost, estimated_rows);
        trigger_cost += cost;

        breakdown.push(TriggerCostItem {
            trigger_name: trigger.name.clone(),
            timing: trigger.timing,
            scope: trigger.scope,
            estimated_cost: cost,
        });
    }

    DmlCostEstimate {
        base_cost,
        trigger_cost,
        total_cost: base_cost + trigger_cost,
        trigger_count: triggers.len(),
        trigger_breakdown: breakdown,
    }
}

fn estimate_trigger_cost(trigger: &TriggerInfo, base_cost: f64, estimated_rows: f64) -> f64 {
    match (trigger.timing, trigger.scope) {
        (TriggerTiming::Before | TriggerTiming::InsteadOf, TriggerScope::Row) => {
            base_cost * BEFORE_ROW_COST * estimated_rows / estimated_rows.max(1.0)
        }
        (TriggerTiming::After, TriggerScope::Row) => {
            base_cost * AFTER_ROW_COST * estimated_rows / estimated_rows.max(1.0)
        }
        (TriggerTiming::Before | TriggerTiming::InsteadOf, TriggerScope::Statement) => {
            base_cost * BEFORE_STMT_COST
        }
        (TriggerTiming::After, TriggerScope::Statement) => base_cost * AFTER_STMT_COST,
    }
}

/// Detect cascading trigger chains in a schema.
///
/// Analyzes trigger action SQL to find triggers that reference
/// other tables that also have triggers, forming chains.
#[must_use]
pub fn detect_cascade(table_name: &str, schema: &SchemaInfo) -> Vec<CascadeWarning> {
    let mut warnings = Vec::new();
    let mut visited = HashSet::new();
    let mut chain = Vec::new();

    detect_cascade_recursive(table_name, schema, &mut visited, &mut chain, &mut warnings);

    warnings
}

fn detect_cascade_recursive(
    table_name: &str,
    schema: &SchemaInfo,
    visited: &mut HashSet<String>,
    chain: &mut Vec<String>,
    warnings: &mut Vec<CascadeWarning>,
) {
    if visited.contains(table_name) {
        chain.push(table_name.to_owned());
        warnings.push(CascadeWarning {
            severity: CascadeSeverity::Error,
            message: format!(
                "Possible infinite trigger loop detected: {}",
                chain.join(" -> ")
            ),
            trigger_chain: chain.clone(),
        });
        chain.pop();
        return;
    }

    let Some(table) = schema.get_table(table_name) else {
        return;
    };

    if table.triggers.is_empty() {
        return;
    }

    visited.insert(table_name.to_owned());
    chain.push(table_name.to_owned());

    let referenced_tables = find_tables_referenced_by_triggers(&table.triggers, schema);

    if referenced_tables.len() > 2 {
        warnings.push(CascadeWarning {
            severity: CascadeSeverity::Warning,
            message: format!(
                "Table {table_name} has triggers referencing \
                 {} other tables: potential fan-out",
                referenced_tables.len()
            ),
            trigger_chain: chain.clone(),
        });
    }

    for ref_table in &referenced_tables {
        detect_cascade_recursive(ref_table, schema, visited, chain, warnings);
    }

    chain.pop();
    visited.remove(table_name);
}

/// Heuristically find table names referenced in trigger action SQL.
fn find_tables_referenced_by_triggers(
    triggers: &[TriggerInfo],
    schema: &SchemaInfo,
) -> Vec<String> {
    let table_names: HashSet<&str> = schema.tables.keys().map(String::as_str).collect();

    let mut referenced = Vec::new();
    for trigger in triggers {
        let upper = trigger.action_sql.to_uppercase();
        for name in &table_names {
            if upper.contains(&name.to_uppercase())
                && *name != trigger.table_name
                && !referenced.contains(&name.to_string())
            {
                referenced.push(name.to_string());
            }
        }
    }
    referenced
}

/// Run a full trigger analysis on a table, returning cost
/// estimates for all DML events and cascade warnings.
#[must_use]
pub fn analyze_table_triggers(
    table_name: &str,
    schema: &SchemaInfo,
    estimated_rows: f64,
) -> TriggerAnalysis {
    let base_cost = 1.0;

    let insert_cost = Some(analyze_dml_cost(
        table_name,
        TriggerEvent::Insert,
        base_cost,
        estimated_rows,
        schema,
    ));
    let update_cost = Some(analyze_dml_cost(
        table_name,
        TriggerEvent::Update,
        base_cost,
        estimated_rows,
        schema,
    ));
    let delete_cost = Some(analyze_dml_cost(
        table_name,
        TriggerEvent::Delete,
        base_cost,
        estimated_rows,
        schema,
    ));

    let cascade_warnings = detect_cascade(table_name, schema);

    TriggerAnalysis {
        table_name: table_name.to_owned(),
        insert_cost,
        update_cost,
        delete_cost,
        cascade_warnings,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;
    use ra_metadata::schema::{
        ColumnInfo, ConstraintInfo, ConstraintKind, DatabaseKind, TableInfo, TriggerEvent,
        TriggerInfo, TriggerScope, TriggerTiming,
    };
    use std::collections::HashMap;

    #[expect(clippy::too_many_lines, reason = "test fixture building schema")]
    fn schema_with_triggers() -> SchemaInfo {
        let mut tables = HashMap::new();

        tables.insert(
            "orders".to_owned(),
            TableInfo {
                name: "orders".to_owned(),
                columns: vec![ColumnInfo {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    ordinal: 1,
                    default_value: None,
                }],
                constraints: vec![ConstraintInfo {
                    name: "orders_pk".to_owned(),
                    kind: ConstraintKind::PrimaryKey,
                    columns: vec!["id".to_owned()],
                    referenced_table: None,
                    referenced_columns: vec![],
                    check_expression: None,
                }],
                indexes: vec![],
                triggers: vec![
                    TriggerInfo {
                        name: "trg_audit_insert".to_owned(),
                        event: TriggerEvent::Insert,
                        timing: TriggerTiming::After,
                        scope: TriggerScope::Row,
                        action_sql: "INSERT INTO audit_log \
                             VALUES (NEW.id)"
                            .to_owned(),
                        table_name: "orders".to_owned(),
                        enabled: true,
                    },
                    TriggerInfo {
                        name: "trg_validate".to_owned(),
                        event: TriggerEvent::Insert,
                        timing: TriggerTiming::Before,
                        scope: TriggerScope::Row,
                        action_sql: "EXECUTE validate_order()".to_owned(),
                        table_name: "orders".to_owned(),
                        enabled: true,
                    },
                    TriggerInfo {
                        name: "trg_update_stats".to_owned(),
                        event: TriggerEvent::Update,
                        timing: TriggerTiming::After,
                        scope: TriggerScope::Statement,
                        action_sql: "UPDATE stats SET count = \
                             (SELECT COUNT(*) FROM orders)"
                            .to_owned(),
                        table_name: "orders".to_owned(),
                        enabled: true,
                    },
                    TriggerInfo {
                        name: "trg_disabled".to_owned(),
                        event: TriggerEvent::Delete,
                        timing: TriggerTiming::After,
                        scope: TriggerScope::Row,
                        action_sql: "DELETE FROM archive".to_owned(),
                        table_name: "orders".to_owned(),
                        enabled: false,
                    },
                ],
                estimated_rows: Some(1000.0),
            },
        );

        tables.insert(
            "audit_log".to_owned(),
            TableInfo {
                name: "audit_log".to_owned(),
                columns: vec![ColumnInfo {
                    name: "id".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    ordinal: 1,
                    default_value: None,
                }],
                constraints: vec![],
                indexes: vec![],
                triggers: vec![TriggerInfo {
                    name: "trg_cascade_audit".to_owned(),
                    event: TriggerEvent::Insert,
                    timing: TriggerTiming::After,
                    scope: TriggerScope::Row,
                    action_sql: "INSERT INTO orders VALUES (1)".to_owned(),
                    table_name: "audit_log".to_owned(),
                    enabled: true,
                }],
                estimated_rows: Some(10000.0),
            },
        );

        tables.insert(
            "stats".to_owned(),
            TableInfo {
                name: "stats".to_owned(),
                columns: vec![ColumnInfo {
                    name: "count".to_owned(),
                    data_type: "integer".to_owned(),
                    nullable: false,
                    ordinal: 1,
                    default_value: None,
                }],
                constraints: vec![],
                indexes: vec![],
                triggers: vec![],
                estimated_rows: Some(1.0),
            },
        );

        SchemaInfo {
            kind: DatabaseKind::PostgreSQL,
            schema_name: "public".to_owned(),
            tables,
        }
    }

    #[test]
    fn dml_cost_with_triggers() {
        let schema = schema_with_triggers();
        let cost = analyze_dml_cost("orders", TriggerEvent::Insert, 10.0, 100.0, &schema);

        assert_eq!(cost.trigger_count, 2);
        assert!(cost.trigger_cost > 0.0);
        assert!(cost.total_cost > cost.base_cost);
        assert_eq!(cost.trigger_breakdown.len(), 2);
    }

    #[test]
    fn dml_cost_no_triggers() {
        let schema = schema_with_triggers();
        let cost = analyze_dml_cost("stats", TriggerEvent::Insert, 10.0, 1.0, &schema);

        assert_eq!(cost.trigger_count, 0);
        assert!(cost.trigger_cost.abs() < f64::EPSILON);
        assert!((cost.total_cost - cost.base_cost).abs() < f64::EPSILON);
    }

    #[test]
    fn dml_cost_disabled_trigger_excluded() {
        let schema = schema_with_triggers();
        let cost = analyze_dml_cost("orders", TriggerEvent::Delete, 10.0, 50.0, &schema);

        assert_eq!(cost.trigger_count, 0);
    }

    #[test]
    fn dml_cost_unknown_table() {
        let schema = schema_with_triggers();
        let cost = analyze_dml_cost("nonexistent", TriggerEvent::Insert, 10.0, 1.0, &schema);

        assert_eq!(cost.trigger_count, 0);
        assert!((cost.total_cost - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dml_cost_statement_trigger() {
        let schema = schema_with_triggers();
        let cost = analyze_dml_cost("orders", TriggerEvent::Update, 10.0, 100.0, &schema);

        assert_eq!(cost.trigger_count, 1);
        assert!(cost.trigger_cost > 0.0);
        let item = &cost.trigger_breakdown[0];
        assert_eq!(item.scope, TriggerScope::Statement);
    }

    #[test]
    fn detect_cascade_loop() {
        let schema = schema_with_triggers();
        let warnings = detect_cascade("orders", &schema);

        let has_error = warnings
            .iter()
            .any(|w| w.severity == CascadeSeverity::Error);
        assert!(has_error, "Should detect circular trigger chain");
    }

    #[test]
    fn detect_cascade_no_triggers() {
        let schema = schema_with_triggers();
        let warnings = detect_cascade("stats", &schema);
        assert!(warnings.is_empty());
    }

    #[test]
    fn detect_cascade_unknown_table() {
        let schema = schema_with_triggers();
        let warnings = detect_cascade("nonexistent", &schema);
        assert!(warnings.is_empty());
    }

    #[test]
    fn full_trigger_analysis() {
        let schema = schema_with_triggers();
        let analysis = analyze_table_triggers("orders", &schema, 100.0);

        assert_eq!(analysis.table_name, "orders");
        assert!(analysis.insert_cost.is_some());
        assert!(analysis.update_cost.is_some());
        assert!(analysis.delete_cost.is_some());

        let insert = analysis.insert_cost.as_ref().unwrap();
        assert_eq!(insert.trigger_count, 2);

        let update = analysis.update_cost.as_ref().unwrap();
        assert_eq!(update.trigger_count, 1);

        let delete = analysis.delete_cost.as_ref().unwrap();
        assert_eq!(delete.trigger_count, 0);
    }

    #[test]
    fn cascade_severity_display() {
        assert_eq!(CascadeSeverity::Info.to_string(), "INFO");
        assert_eq!(CascadeSeverity::Warning.to_string(), "WARNING");
        assert_eq!(CascadeSeverity::Error.to_string(), "ERROR");
    }

    #[test]
    fn find_tables_referenced_basic() {
        let schema = schema_with_triggers();
        let table = schema.get_table("orders").unwrap();
        let refs = find_tables_referenced_by_triggers(&table.triggers, &schema);

        assert!(refs.contains(&"audit_log".to_string()));
    }
}

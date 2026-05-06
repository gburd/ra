//! SQL storyline patterns for lifecycle-aware testing.
//!
//! Models sequences of SQL operations that form coherent database
//! workflows. A storyline represents the lifecycle of a table:
//! create -> insert -> query -> update -> delete -> drop.
//!
//! Storylines test the optimizer's behavior across related
//! statements, catching edge cases that isolated query testing misses.

use proptest::prelude::*;
use ra_core::algebra::RelExpr;

use crate::generator::{arb_rel_expr, arb_simple_predicate};

/// A named stage in the SQL lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorylineStage {
    /// Table creation (schema definition).
    Create,
    /// Data insertion.
    Insert,
    /// Query execution (SELECT).
    Query,
    /// Data modification (UPDATE).
    Update,
    /// Data removal (DELETE).
    Delete,
    /// Table removal (DROP).
    Drop,
}

impl StorylineStage {
    /// Return all stages in lifecycle order.
    #[must_use]
    pub fn lifecycle_order() -> &'static [Self] {
        &[
            Self::Create,
            Self::Insert,
            Self::Query,
            Self::Update,
            Self::Delete,
            Self::Drop,
        ]
    }
}

impl std::fmt::Display for StorylineStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create => write!(f, "CREATE"),
            Self::Insert => write!(f, "INSERT"),
            Self::Query => write!(f, "QUERY"),
            Self::Update => write!(f, "UPDATE"),
            Self::Delete => write!(f, "DELETE"),
            Self::Drop => write!(f, "DROP"),
        }
    }
}

/// A pattern describing which lifecycle stages to include in a
/// storyline and any constraints on their relationships.
#[derive(Debug, Clone)]
pub struct StorylinePattern {
    /// Stages to include in order.
    stages: Vec<StorylineStage>,
    /// Number of queries to generate per stage.
    queries_per_stage: usize,
    /// Table names to use across the storyline.
    table_names: Vec<String>,
}

impl StorylinePattern {
    /// Full lifecycle: create -> insert -> query -> update -> delete -> drop.
    #[must_use]
    pub fn full_lifecycle() -> Self {
        Self {
            stages: StorylineStage::lifecycle_order().to_vec(),
            queries_per_stage: 3,
            table_names: vec![
                "test_table".to_owned(),
                "ref_table".to_owned(),
            ],
        }
    }

    /// Read-heavy workload: many queries with occasional updates.
    #[must_use]
    pub fn read_heavy() -> Self {
        Self {
            stages: vec![
                StorylineStage::Create,
                StorylineStage::Insert,
                StorylineStage::Query,
                StorylineStage::Query,
                StorylineStage::Query,
                StorylineStage::Query,
                StorylineStage::Query,
                StorylineStage::Update,
                StorylineStage::Query,
                StorylineStage::Query,
            ],
            queries_per_stage: 5,
            table_names: vec![
                "users".to_owned(),
                "orders".to_owned(),
                "products".to_owned(),
            ],
        }
    }

    /// Write-heavy workload: many inserts/updates with occasional reads.
    #[must_use]
    pub fn write_heavy() -> Self {
        Self {
            stages: vec![
                StorylineStage::Create,
                StorylineStage::Insert,
                StorylineStage::Insert,
                StorylineStage::Insert,
                StorylineStage::Update,
                StorylineStage::Update,
                StorylineStage::Query,
                StorylineStage::Delete,
                StorylineStage::Insert,
            ],
            queries_per_stage: 3,
            table_names: vec![
                "events".to_owned(),
                "metrics".to_owned(),
            ],
        }
    }

    /// Mixed DML workload: interleaved reads and writes.
    #[must_use]
    pub fn mixed_dml() -> Self {
        Self {
            stages: vec![
                StorylineStage::Create,
                StorylineStage::Insert,
                StorylineStage::Query,
                StorylineStage::Update,
                StorylineStage::Query,
                StorylineStage::Delete,
                StorylineStage::Query,
                StorylineStage::Insert,
                StorylineStage::Query,
            ],
            queries_per_stage: 2,
            table_names: vec![
                "accounts".to_owned(),
                "transactions".to_owned(),
            ],
        }
    }

    /// Custom pattern from stage list.
    #[must_use]
    pub fn custom(
        stages: Vec<StorylineStage>,
        queries_per_stage: usize,
        table_names: Vec<String>,
    ) -> Self {
        Self {
            stages,
            queries_per_stage,
            table_names,
        }
    }

    /// Return the stages in this pattern.
    #[must_use]
    pub fn stages(&self) -> &[StorylineStage] {
        &self.stages
    }

    /// Return the table names used by this pattern.
    #[must_use]
    pub fn table_names(&self) -> &[String] {
        &self.table_names
    }

    /// Return the number of queries to generate per stage.
    #[must_use]
    pub fn queries_per_stage(&self) -> usize {
        self.queries_per_stage
    }
}

/// A step in a storyline: a stage paired with a relational expression
/// representing the optimizer input for that step.
#[derive(Debug, Clone)]
pub struct StorylineStep {
    /// The lifecycle stage.
    pub stage: StorylineStage,
    /// The relational expression to optimize.
    pub expr: RelExpr,
    /// Description of this step.
    pub description: String,
}

/// A complete storyline: a sequence of steps exercising the optimizer
/// across a table lifecycle.
#[derive(Debug, Clone)]
pub struct SqlStoryline {
    /// The pattern this storyline was generated from.
    pub pattern: StorylinePattern,
    /// The ordered steps.
    pub steps: Vec<StorylineStep>,
}

impl SqlStoryline {
    /// Number of steps in the storyline.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the storyline has zero steps.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// Generate a storyline step for a Query stage.
///
/// Produces varied query patterns: simple selects, filtered queries,
/// joins, aggregates, and set operations.
fn arb_query_step(
    table: String,
) -> impl Strategy<Value = StorylineStep> {
    arb_rel_expr(2).prop_map(move |expr| StorylineStep {
        stage: StorylineStage::Query,
        expr,
        description: format!("query against {table}"),
    })
}

/// Generate a scan for a given table name.
fn scan_for(table: &str) -> RelExpr {
    RelExpr::Scan {
        table: table.to_owned(),
        alias: None,
    }
}

/// Generate a storyline step for an Insert stage.
///
/// Models INSERT as a Project over Values (the optimizer sees the
/// source query of INSERT ... SELECT).
fn arb_insert_step(
    table: String,
) -> impl Strategy<Value = StorylineStep> {
    arb_rel_expr(1).prop_map(move |source| {
        StorylineStep {
            stage: StorylineStage::Insert,
            expr: source,
            description: format!("insert into {table}"),
        }
    })
}

/// Generate a storyline step for an Update stage.
///
/// Models UPDATE as a Filter over a Scan (the WHERE clause of the
/// UPDATE determines which rows change).
fn arb_update_step(
    table: String,
) -> impl Strategy<Value = StorylineStep> {
    arb_simple_predicate().prop_map(move |pred| {
        StorylineStep {
            stage: StorylineStage::Update,
            expr: RelExpr::Filter {
                predicate: pred,
                input: Box::new(scan_for(&table)),
            },
            description: format!("update {table}"),
        }
    })
}

/// Generate a storyline step for a Delete stage.
///
/// Models DELETE as a Filter over a Scan.
fn arb_delete_step(
    table: String,
) -> impl Strategy<Value = StorylineStep> {
    arb_simple_predicate().prop_map(move |pred| {
        StorylineStep {
            stage: StorylineStage::Delete,
            expr: RelExpr::Filter {
                predicate: pred,
                input: Box::new(scan_for(&table)),
            },
            description: format!("delete from {table}"),
        }
    })
}

/// Generate a complete storyline from a pattern.
///
/// Each stage in the pattern generates one or more relational
/// expressions that the optimizer should be able to handle.
pub fn arb_storyline(
    pattern: StorylinePattern,
) -> impl Strategy<Value = SqlStoryline> {
    let table_names = pattern.table_names.clone();
    let stages = pattern.stages.clone();

    let step_strategies: Vec<_> = stages
        .iter()
        .map(|stage| {
            let table =
                table_names.first().cloned().unwrap_or_else(|| {
                    "default_table".to_owned()
                });
            match stage {
                StorylineStage::Create | StorylineStage::Drop => {
                    let stage_copy = *stage;
                    let table_copy = table.clone();
                    Just(StorylineStep {
                        stage: stage_copy,
                        expr: scan_for(&table_copy),
                        description: format!(
                            "{stage_copy} {table_copy}"
                        ),
                    })
                    .boxed()
                }
                StorylineStage::Insert => {
                    arb_insert_step(table).boxed()
                }
                StorylineStage::Query => {
                    arb_query_step(table).boxed()
                }
                StorylineStage::Update => {
                    arb_update_step(table).boxed()
                }
                StorylineStage::Delete => {
                    arb_delete_step(table).boxed()
                }
            }
        })
        .collect();

    step_strategies.prop_map(move |steps| SqlStoryline {
        pattern: pattern.clone(),
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    #[test]
    fn full_lifecycle_generates_all_stages() {
        // Spawn with a 32 MB stack — arb_storyline generates deeply nested
        // RelExpr trees that can overflow the default 8 MB test thread stack.
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let pattern = StorylinePattern::full_lifecycle();
                let mut runner = TestRunner::default();
                let storyline = arb_storyline(pattern)
                    .new_tree(&mut runner)
                    .expect("generate storyline")
                    .current();

                assert_eq!(storyline.len(), 6);

                let stages: Vec<_> =
                    storyline.steps.iter().map(|s| s.stage).collect();
                assert_eq!(stages[0], StorylineStage::Create);
                assert_eq!(stages[1], StorylineStage::Insert);
                assert_eq!(stages[2], StorylineStage::Query);
                assert_eq!(stages[3], StorylineStage::Update);
                assert_eq!(stages[4], StorylineStage::Delete);
                assert_eq!(stages[5], StorylineStage::Drop);
            })
            .expect("spawn test thread")
            .join()
            .expect("test thread panicked");
    }

    #[test]
    fn read_heavy_has_many_queries() {
        let pattern = StorylinePattern::read_heavy();
        let query_count = pattern
            .stages()
            .iter()
            .filter(|s| **s == StorylineStage::Query)
            .count();
        assert!(
            query_count >= 5,
            "read-heavy should have >= 5 query stages"
        );
    }

    #[test]
    fn storyline_stage_display() {
        assert_eq!(format!("{}", StorylineStage::Create), "CREATE");
        assert_eq!(format!("{}", StorylineStage::Query), "QUERY");
    }
}

//! `PostgreSQL` API bridge for applying plan advice.
//!
//! Supports three backends:
//! - `pg_plan_advice` GUC (`PostgreSQL` 19+)
//! - `pg_stash_advice` DSM (`PostgreSQL` 19+, future)
//! - `pg_hint_plan` comment injection (`PostgreSQL` 15-18 fallback)

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::advice_gen::{AdviceGenerator, PlanAdvice};

/// Errors from the `PostgreSQL` advice API.
#[derive(Debug, Error)]
pub enum PgAdviceError {
    /// Failed to set advice via GUC.
    #[error("failed to set GUC pg_plan_advice.advice: {0}")]
    GucSetFailed(String),

    /// Failed to stash advice in DSM.
    #[error("failed to stash advice: {0}")]
    StashFailed(String),

    /// The requested backend is not available.
    #[error("backend not available: {0}")]
    BackendUnavailable(String),

    /// Connection to `PostgreSQL` failed.
    #[error("PostgreSQL connection error: {0}")]
    ConnectionError(String),
}

/// Which mechanism to use for applying advice.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash,
    Serialize, Deserialize,
)]
pub enum AdviceBackend {
    /// Set `pg_plan_advice.advice` GUC per session.
    PgPlanAdviceGuc,
    /// Store in DSM via `pg_stash_advice` (auto-applied by query ID).
    PgStashAdvice,
    /// Rewrite SQL with `pg_hint_plan` comment hints.
    PgHintPlan,
}

impl fmt::Display for AdviceBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PgPlanAdviceGuc => {
                write!(f, "pg_plan_advice (GUC)")
            }
            Self::PgStashAdvice => {
                write!(f, "pg_stash_advice (DSM)")
            }
            Self::PgHintPlan => write!(f, "pg_hint_plan"),
        }
    }
}

/// Formats advice for a specific backend without executing it.
///
/// This is the pure-logic layer that generates the SQL statements
/// or modified queries needed to apply advice. Actual execution
/// against `PostgreSQL` is handled by the daemon.
pub struct PgAdviceApi {
    backend: AdviceBackend,
}

impl PgAdviceApi {
    /// Create an API targeting the given backend.
    #[must_use]
    pub fn new(backend: AdviceBackend) -> Self {
        Self { backend }
    }

    /// Which backend this API uses.
    #[must_use]
    pub fn backend(&self) -> AdviceBackend {
        self.backend
    }

    /// Format the SQL statement to apply advice.
    ///
    /// For `PgPlanAdviceGuc`: returns a `SET` statement.
    /// For `PgStashAdvice`: returns a `SELECT pg_set_stashed_advice(...)` call.
    /// For `PgHintPlan`: returns the original query with hint comment prepended.
    #[must_use]
    pub fn format_apply_sql(
        &self,
        advice: &[PlanAdvice],
        original_query: Option<&str>,
    ) -> String {
        match self.backend {
            AdviceBackend::PgPlanAdviceGuc => {
                let combined =
                    AdviceGenerator::combine_advice(advice);
                format_guc_set(&combined)
            }
            AdviceBackend::PgStashAdvice => {
                let combined =
                    AdviceGenerator::combine_advice(advice);
                let query_id = advice
                    .first()
                    .map_or("unknown", |a| a.query_id.as_str());
                format_stash_call(query_id, &combined)
            }
            AdviceBackend::PgHintPlan => {
                let hint =
                    AdviceGenerator::to_pg_hint_plan(advice);
                match original_query {
                    Some(q) => format!("{hint} {q}"),
                    None => hint,
                }
            }
        }
    }

    /// Format a SQL statement to clear advice for the session.
    #[must_use]
    pub fn format_clear_sql(&self) -> String {
        match self.backend {
            AdviceBackend::PgPlanAdviceGuc => {
                "RESET pg_plan_advice.advice".to_string()
            }
            AdviceBackend::PgStashAdvice => {
                "SELECT pg_clear_advice_stash()".to_string()
            }
            AdviceBackend::PgHintPlan => String::new(),
        }
    }

    /// Format a SQL statement to check if the backend extension
    /// is available.
    #[must_use]
    pub fn format_availability_check(&self) -> String {
        match self.backend {
            AdviceBackend::PgPlanAdviceGuc => {
                "SHOW pg_plan_advice.advice".to_string()
            }
            AdviceBackend::PgStashAdvice => {
                "SELECT 1 FROM pg_available_extensions \
                 WHERE name = 'pg_stash_advice'"
                    .to_string()
            }
            AdviceBackend::PgHintPlan => {
                "SELECT 1 FROM pg_available_extensions \
                 WHERE name = 'pg_hint_plan'"
                    .to_string()
            }
        }
    }

    /// Detect the best available backend by returning the SQL
    /// checks to run in priority order.
    #[must_use]
    pub fn detection_order() -> Vec<(AdviceBackend, String)> {
        vec![
            (
                AdviceBackend::PgPlanAdviceGuc,
                "SHOW pg_plan_advice.advice".to_string(),
            ),
            (
                AdviceBackend::PgStashAdvice,
                "SELECT 1 FROM pg_available_extensions \
                 WHERE name = 'pg_stash_advice'"
                    .to_string(),
            ),
            (
                AdviceBackend::PgHintPlan,
                "SELECT 1 FROM pg_available_extensions \
                 WHERE name = 'pg_hint_plan'"
                    .to_string(),
            ),
        ]
    }
}

/// Format a `SET pg_plan_advice.advice = '...'` statement.
fn format_guc_set(advice: &str) -> String {
    let escaped = advice.replace('\'', "''");
    format!("SET pg_plan_advice.advice = '{escaped}'")
}

/// Format a `pg_set_stashed_advice(...)` call.
fn format_stash_call(query_id: &str, advice: &str) -> String {
    let escaped_id = query_id.replace('\'', "''");
    let escaped_advice = advice.replace('\'', "''");
    format!(
        "SELECT pg_set_stashed_advice('ra_stash', \
         '{escaped_id}', '{escaped_advice}')"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advice_gen::AdviceType;

    fn sample_advice() -> Vec<PlanAdvice> {
        vec![
            PlanAdvice {
                query_id: "q123".into(),
                advice_type: AdviceType::JoinReordering,
                advice_string: "JOIN_ORDER(a b c)".into(),
                estimated_improvement: 2.0,
                confidence: 0.9,
            },
            PlanAdvice {
                query_id: "q123".into(),
                advice_type: AdviceType::JoinMethod,
                advice_string: "HASH_JOIN(b)".into(),
                estimated_improvement: 1.5,
                confidence: 0.8,
            },
        ]
    }

    #[test]
    fn guc_format() {
        let api = PgAdviceApi::new(AdviceBackend::PgPlanAdviceGuc);
        let sql = api.format_apply_sql(&sample_advice(), None);
        assert_eq!(
            sql,
            "SET pg_plan_advice.advice = \
             'JOIN_ORDER(a b c) HASH_JOIN(b)'"
        );
    }

    #[test]
    fn guc_escapes_quotes() {
        let api = PgAdviceApi::new(AdviceBackend::PgPlanAdviceGuc);
        let advice = vec![PlanAdvice {
            query_id: "q1".into(),
            advice_type: AdviceType::JoinMethod,
            advice_string: "HASH_JOIN(t'x)".into(),
            estimated_improvement: 1.0,
            confidence: 0.5,
        }];
        let sql = api.format_apply_sql(&advice, None);
        assert!(sql.contains("t''x"));
    }

    #[test]
    fn stash_format() {
        let api = PgAdviceApi::new(AdviceBackend::PgStashAdvice);
        let sql = api.format_apply_sql(&sample_advice(), None);
        assert!(sql.contains("pg_set_stashed_advice"));
        assert!(sql.contains("q123"));
        assert!(sql.contains("JOIN_ORDER(a b c) HASH_JOIN(b)"));
    }

    #[test]
    fn hint_plan_format() {
        let api = PgAdviceApi::new(AdviceBackend::PgHintPlan);
        let sql = api.format_apply_sql(
            &sample_advice(),
            Some("SELECT * FROM a JOIN b ON a.id = b.id"),
        );
        assert!(sql.starts_with("/*+"));
        assert!(sql.contains("Leading(a b c)"));
        assert!(sql.contains("HashJoin(b)"));
        assert!(sql.ends_with(
            "SELECT * FROM a JOIN b ON a.id = b.id"
        ));
    }

    #[test]
    fn clear_sql_guc() {
        let api = PgAdviceApi::new(AdviceBackend::PgPlanAdviceGuc);
        assert_eq!(
            api.format_clear_sql(),
            "RESET pg_plan_advice.advice"
        );
    }

    #[test]
    fn clear_sql_stash() {
        let api = PgAdviceApi::new(AdviceBackend::PgStashAdvice);
        assert!(api
            .format_clear_sql()
            .contains("pg_clear_advice_stash"));
    }

    #[test]
    fn clear_sql_hint_plan() {
        let api = PgAdviceApi::new(AdviceBackend::PgHintPlan);
        assert!(api.format_clear_sql().is_empty());
    }

    #[test]
    fn detection_order_priority() {
        let order = PgAdviceApi::detection_order();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0].0, AdviceBackend::PgPlanAdviceGuc);
        assert_eq!(order[1].0, AdviceBackend::PgStashAdvice);
        assert_eq!(order[2].0, AdviceBackend::PgHintPlan);
    }

    #[test]
    fn backend_display() {
        assert_eq!(
            format!("{}", AdviceBackend::PgPlanAdviceGuc),
            "pg_plan_advice (GUC)"
        );
        assert_eq!(
            format!("{}", AdviceBackend::PgHintPlan),
            "pg_hint_plan"
        );
    }

    #[test]
    fn availability_check_guc() {
        let api = PgAdviceApi::new(AdviceBackend::PgPlanAdviceGuc);
        assert_eq!(
            api.format_availability_check(),
            "SHOW pg_plan_advice.advice"
        );
    }

    #[test]
    fn availability_check_hint_plan() {
        let api = PgAdviceApi::new(AdviceBackend::PgHintPlan);
        assert!(api
            .format_availability_check()
            .contains("pg_hint_plan"));
    }
}

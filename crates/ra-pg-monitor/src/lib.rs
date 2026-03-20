//! PostgreSQL monitoring, schema analysis, and configuration advisory.
//!
//! Provides an OtterTune-like system for:
//! - **Query monitoring**: track execution times, plans, costs, regressions
//! - **Schema analysis**: unused/missing/duplicate indexes, bloat, FK issues
//! - **Configuration tuning**: memory, planner, parallelism recommendations
//! - **Statistics freshness**: staleness detection and ANALYZE suggestions
//! - **Cardinality errors**: q-error tracking and estimation feedback
//! - **TUI dashboard**: real-time monitoring with actionable advice

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

pub mod bloat_detector;
pub mod config_checker;
pub mod error_detection;
pub mod monitor_tui;
pub mod query_monitor;
pub mod recommendations;
pub mod schema_analyzer;
pub mod stats_staleness;

pub use bloat_detector::{BloatDetector, BloatInfo, BloatSeverity};
pub use config_checker::{ConfigChecker, ConfigIssue, PgConfig};
pub use error_detection::{ErrorDetector, TableErrorSummary};
pub use monitor_tui::MonitorApp;
pub use query_monitor::{QueryMonitor, QueryRecord, QuerySeverity};
pub use recommendations::{Advisor, Recommendation, Severity};
pub use schema_analyzer::{SchemaAnalyzer, SchemaIssue, SchemaIssueKind};
pub use stats_staleness::{StalenessChecker, StalenessInfo, StalenessLevel};

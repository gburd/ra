//! `PostgreSQL` plan advisor daemon.
//!
//! Generates `pg_plan_advice` hints from RA optimizer output. Supports:
//! - Monitoring slow queries via `pg_stat_statements`
//! - Converting RA optimization results to advice strings
//! - Applying advice via GUC (`pg_plan_advice.advice`)
//! - Fallback to `pg_hint_plan` format for `PostgreSQL` 15-18

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

pub mod advice_gen;
pub mod daemon;
pub mod monitor;
pub mod pg_api;

pub use advice_gen::{AdviceGenerator, PlanAdvice};
pub use daemon::{AdvisorDaemon, DaemonConfig};
pub use monitor::{QueryCandidate, QueryTracker};
pub use pg_api::{AdviceBackend, PgAdviceApi};

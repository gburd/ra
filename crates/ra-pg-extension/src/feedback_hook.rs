//! Executor end hook: captures execution feedback for the neural cost model.
//!
//! Implements the feedback loop between plan execution and model training:
//!
//! ```text
//! planner_hook (planning phase)
//!     → stores PendingFeedback in PENDING_QUERIES
//!
//! executor_end_hook (execution phase)
//!     → reads actual timing/buffers from instrumentation
//!     → constructs ExecutionFeedback
//!     → pushes to FeedbackCollector
//!     → triggers OnlineLearner training when batch is full
//! ```
//!
//! The executor hook overhead is <1us per query: a hash lookup, counter
//! reads, and a push to a pre-allocated ring buffer.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;
use pgrx::prelude::*;

use ra_engine::cost_model::{ExecutionFeedback, FeedbackCollector, QueryFeatures};
use ra_engine::state::SystemFingerprint;
use ra_engine::training_coordinator::{shared_coordinator, SharedTrainingCoordinator};

/// Batch size threshold: trigger training after this many samples
/// accumulate in the FeedbackCollector.
const TRAINING_BATCH_THRESHOLD: usize = 64;

/// Per-query tracking data stored between planning and execution end.
pub struct PendingFeedback {
    /// Hash of the normalized query text.
    pub query_fingerprint: u64,
    /// Hash of the plan structure.
    pub plan_fingerprint: u64,
    /// Features extracted from the RelExpr at plan time.
    pub features: QueryFeatures,
    /// System fingerprint snapshot at plan time.
    pub system_fingerprint: SystemFingerprint,
    /// Cost predicted by the neural model at plan time.
    pub predicted_cost: f64,
    /// Which rule groups were fired during optimization.
    pub rules_fired: Vec<u32>,
    /// Total rule groups enabled for this query.
    pub rules_enabled: u32,
    /// Wall-clock time at start of execution (for fallback timing).
    pub exec_start: Instant,
}

/// Pending feedback keyed by `queryId` (from `pg_sys::Query::queryId`).
///
/// Access is single-threaded (one backend process), but we use Mutex for
/// safety with pgrx's panic-catch model.
static PENDING_QUERIES: Lazy<Mutex<HashMap<u64, PendingFeedback>>> =
    Lazy::new(|| Mutex::new(HashMap::with_capacity(16)));

/// Global feedback collector and online learner.
static FEEDBACK_STATE: Lazy<Mutex<FeedbackState>> = Lazy::new(|| Mutex::new(FeedbackState::new()));

struct FeedbackState {
    collector: FeedbackCollector,
    training_coordinator: SharedTrainingCoordinator,
}

impl FeedbackState {
    fn new() -> Self {
        Self {
            collector: FeedbackCollector::new(),
            training_coordinator: shared_coordinator(),
        }
    }
}

/// Saved pointer to the previous ExecutorEnd hook.
static mut PREV_EXECUTOR_END_HOOK: pg_sys::ExecutorEnd_hook_type = None;

/// Register the executor end hook during extension initialization.
pub fn register_hooks() {
    unsafe {
        PREV_EXECUTOR_END_HOOK = pg_sys::ExecutorEnd_hook;
        pg_sys::ExecutorEnd_hook = Some(ra_executor_end_hook);
    }
}

/// Store pending feedback for a query entering execution.
///
/// Called from `planner_hook` after successful RA optimization. The
/// `query_id` should be the `pg_sys::Query::queryId` field.
pub fn register_pending(query_id: u64, pending: PendingFeedback) {
    if let Ok(mut map) = PENDING_QUERIES.lock() {
        // Evict stale entries if map grows too large (leak prevention).
        if map.len() > 256 {
            // Remove oldest entries (queries that never reached executor end).
            let keys: Vec<u64> = map.keys().copied().collect();
            for key in keys.iter().take(128) {
                map.remove(key);
            }
        }
        map.insert(query_id, pending);
    }
}

/// The executor end hook entry point.
///
/// # Safety
///
/// Called by PostgreSQL's executor infrastructure with a valid
/// `QueryDesc` pointer.
#[pg_guard]
unsafe extern "C-unwind" fn ra_executor_end_hook(query_desc: *mut pg_sys::QueryDesc) {
    // Capture feedback before calling the previous hook (which may free state).
    if !query_desc.is_null() {
        capture_feedback(query_desc);
    }

    // Chain to previous hook or standard executor end.
    if let Some(prev) = PREV_EXECUTOR_END_HOOK {
        prev(query_desc);
    } else {
        pg_sys::standard_ExecutorEnd(query_desc);
    }
}

/// Extract execution metrics from the QueryDesc and feed to the collector.
///
/// # Safety
///
/// Caller must pass a valid `QueryDesc` pointer with initialized fields.
unsafe fn capture_feedback(query_desc: *mut pg_sys::QueryDesc) {
    // Get the queryId to look up pending feedback.
    let planned_stmt = (*query_desc).plannedstmt;
    if planned_stmt.is_null() {
        return;
    }
    let query_id = (*planned_stmt).queryId as u64;
    if query_id == 0 {
        return;
    }

    // Look up the pending feedback registered during planning.
    let pending = {
        let Ok(mut map) = PENDING_QUERIES.lock() else {
            return;
        };
        map.remove(&query_id)
    };

    let Some(pending) = pending else {
        return; // Not an RA-optimized query, skip.
    };

    // Extract actual execution metrics.
    let (actual_time_ms, actual_rows, buffers_hit, buffers_read) =
        extract_execution_metrics(query_desc, &pending);

    let feedback = ExecutionFeedback {
        query_fingerprint: pending.query_fingerprint,
        plan_fingerprint: pending.plan_fingerprint,
        features: pending.features.clone(),
        system_fingerprint: pending.system_fingerprint,
        predicted_cost: pending.predicted_cost,
        actual_time_ms,
        actual_rows,
        buffers_hit,
        buffers_read,
        rules_fired: pending.rules_fired,
        rules_enabled: pending.rules_enabled,
    };

    // Feed to the collector and update model accuracy tracking.
    if let Ok(mut state) = FEEDBACK_STATE.lock() {
        state.collector.record(feedback.clone());

        // Drain batch and feed to training coordinator when threshold is reached.
        if state.collector.buffered_count() >= TRAINING_BATCH_THRESHOLD {
            let batch = state.collector.drain();

            // Feed each feedback sample to the training coordinator.
            if let Ok(mut coord) = state.training_coordinator.lock() {
                for item in &batch {
                    coord.record_feedback(&item.features, item.actual_time_ms);
                }
            }

            // Update fingerprint with current model accuracy.
            let total = state.collector.total_processed();
            let mape = state.collector.current_mape();
            update_fingerprint_model_stats(total, mape);
        }
    }
}

/// Extract timing, row counts, and buffer stats from the executor state.
///
/// # Safety
///
/// Caller must pass a valid `QueryDesc` pointer.
unsafe fn extract_execution_metrics(
    query_desc: *mut pg_sys::QueryDesc,
    pending: &PendingFeedback,
) -> (f64, u64, u64, u64) {
    let plan_state = (*query_desc).planstate;

    if plan_state.is_null() || (*plan_state).instrument.is_null() {
        // No instrumentation: fall back to wall-clock timing.
        let elapsed = pending.exec_start.elapsed();
        return (elapsed.as_secs_f64() * 1000.0, 0, 0, 0);
    }

    let instrument = &*(*plan_state).instrument;

    // Total execution time in ms (from PostgreSQL's Instrumentation struct).
    // PG19 changed Instrumentation.total from a double (seconds) to an
    // instr_time (nanoseconds in `.ticks`).
    #[cfg(not(feature = "pg19"))]
    let total_time_ms = instrument.total * 1000.0;
    #[cfg(feature = "pg19")]
    let total_time_ms = instrument.total.ticks as f64 / 1_000_000.0;

    // Total rows produced by the top-level plan node.
    let actual_rows = instrument.ntuples as u64;

    // Buffer usage from the plan state.
    let buffers_hit = instrument.bufusage.shared_blks_hit as u64;
    let buffers_read = instrument.bufusage.shared_blks_read as u64;

    (total_time_ms, actual_rows, buffers_hit, buffers_read)
}

/// Update the SystemFingerprint with latest model training stats.
fn update_fingerprint_model_stats(total_samples: u64, mape: f32) {
    let reader = crate::monitor::fingerprint_reader();
    let mut fp = reader.read();
    fp.model_samples_trained = total_samples.min(u32::MAX as u64) as u32;
    fp.model_recent_mape = mape;
    reader.update(fp);
}

/// Get current collector statistics for diagnostics.
///
/// Returns (total_processed, buffered_count, current_mape, training_steps).
pub fn collector_stats() -> (u64, usize, f32, u64) {
    if let Ok(state) = FEEDBACK_STATE.lock() {
        let training_steps = state
            .training_coordinator
            .lock()
            .ok()
            .map(|c| c.stats().total_train_steps as u64)
            .unwrap_or(0);
        (
            state.collector.total_processed(),
            state.collector.buffered_count(),
            state.collector.current_mape(),
            training_steps,
        )
    } else {
        (0, 0, 1.0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn training_thresholds_are_consistent() {
        assert!(TRAINING_BATCH_THRESHOLD > 0);
        // Batch threshold must be reasonable (not too large for latency,
        // not too small for training stability).
        assert!(TRAINING_BATCH_THRESHOLD <= 256);
    }

    #[test]
    fn feedback_state_initializes() {
        let state = FeedbackState::new();
        assert_eq!(state.collector.total_processed(), 0);
    }
}

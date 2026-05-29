//! RFC 0026 (Adaptive Cost Calibration) integration tests.
//!
//! Verifies that the optimizer-side surface for the
//! [`AdaptiveCalibrator`] works end-to-end: attach a calibrator,
//! ingest feedback, query correction factors.
//!
//! The full RFC 0026 implementation also wires correction
//! factors into the internal cost function. Today the optimizer
//! exposes the calibrator via [`Optimizer::correction_factor`]
//! and [`Optimizer::adjust_cost`] so downstream consumers can
//! query it; folding it into the egg cost function is tracked
//! separately (changing the cost function affects every
//! existing test and benchmark, so it's gated behind a
//! follow-up).

#![expect(
    clippy::float_cmp,
    reason = "test code; correction factors are exact 1.0 sentinels or computed values compared within asserts"
)]

use std::sync::{Arc, RwLock};

use ra_engine::{
    AdaptiveCalibrator, CalibrationConfig, CostFeedback, OperatorKind, Optimizer,
};

#[test]
fn no_calibrator_returns_neutral_correction() {
    let opt = Optimizer::new();
    assert_eq!(opt.correction_factor(OperatorKind::HashJoin), 1.0);
    assert_eq!(opt.adjust_cost(OperatorKind::HashJoin, 100.0), 100.0);
}

#[test]
fn calibrator_starts_neutral_until_bias_detected() {
    // A fresh calibrator hasn't seen any feedback. Even after
    // attaching it, correction_factor is 1.0 until min_samples
    // observations show systematic bias.
    let cal = Arc::new(RwLock::new(AdaptiveCalibrator::with_config(
        CalibrationConfig::default(),
    )));
    let mut opt = Optimizer::new();
    opt.set_calibrator(Arc::clone(&cal));
    assert_eq!(opt.correction_factor(OperatorKind::HashJoin), 1.0);
}

#[test]
fn calibrator_applies_correction_after_consistent_bias() {
    // Build a calibrator and feed it a stream of HashJoin
    // observations where actual cost is consistently 2x the
    // estimate. After enough samples, correction_factor should
    // converge toward 2.0.
    let config = CalibrationConfig::default();
    let cal = Arc::new(RwLock::new(AdaptiveCalibrator::with_config(config)));
    let mut opt = Optimizer::new();
    opt.set_calibrator(Arc::clone(&cal));

    // Feed 30 observations: estimated_cost=100, actual_cost=200
    let feedback: Vec<CostFeedback> = (0..30)
        .map(|_| CostFeedback {
            operator: OperatorKind::HashJoin,
            estimated_cost: 100.0,
            actual_cost: 200.0,
            estimated_rows: 100.0,
            actual_rows: 100.0,
        })
        .collect();
    opt.ingest_calibration_feedback(&feedback);

    // After ingestion, correction_factor should be > 1.0
    // (closer to 2.0). The exact value depends on EWMA alpha
    // and bias-detection threshold; we check it's in a
    // reasonable range rather than equal to 2.0.
    let factor = opt.correction_factor(OperatorKind::HashJoin);
    assert!(
        factor > 1.5 && factor <= 2.5,
        "expected correction factor ~2.0 after 2x bias; got {factor}",
    );
}

#[test]
fn calibrator_does_not_correct_other_operators() {
    // Bias is per-operator. Feeding HashJoin observations
    // shouldn't affect MergeJoin's correction factor.
    let cal = Arc::new(RwLock::new(AdaptiveCalibrator::with_config(
        CalibrationConfig::default(),
    )));
    let mut opt = Optimizer::new();
    opt.set_calibrator(Arc::clone(&cal));

    let feedback: Vec<CostFeedback> = (0..30)
        .map(|_| CostFeedback {
            operator: OperatorKind::HashJoin,
            estimated_cost: 100.0,
            actual_cost: 300.0,
            estimated_rows: 100.0,
            actual_rows: 100.0,
        })
        .collect();
    opt.ingest_calibration_feedback(&feedback);

    // MergeJoin had no observations — should still be 1.0.
    assert_eq!(opt.correction_factor(OperatorKind::MergeJoin), 1.0);
}

#[test]
fn adjust_cost_multiplies_by_correction_factor() {
    let cal = Arc::new(RwLock::new(AdaptiveCalibrator::with_config(
        CalibrationConfig::default(),
    )));
    let mut opt = Optimizer::new();
    opt.set_calibrator(Arc::clone(&cal));

    // Feed bias for IndexScan
    let feedback: Vec<CostFeedback> = (0..30)
        .map(|_| CostFeedback {
            operator: OperatorKind::IndexScan,
            estimated_cost: 50.0,
            actual_cost: 100.0,
            estimated_rows: 50.0,
            actual_rows: 50.0,
        })
        .collect();
    opt.ingest_calibration_feedback(&feedback);

    let factor = opt.correction_factor(OperatorKind::IndexScan);
    let base = 1000.0;
    assert!(
        (opt.adjust_cost(OperatorKind::IndexScan, base) - (base * factor)).abs() < 1e-9,
        "adjust_cost should multiply by correction_factor",
    );
}

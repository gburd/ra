//! Hardware-adaptive test calibration utilities for the RA optimizer.
//!
//! This crate provides test profiles that automatically scale timing expectations,
//! iteration limits, and resource constraints based on the underlying hardware
//! capabilities. Tests use calibrated profiles to avoid false failures on slower
//! hardware while still catching performance regressions.

pub mod calibrate;
pub mod profile;
// Phase 6: Timeline system (deferred)
// pub mod timeline_helpers;

pub use calibrate::calibrate;
pub use profile::{CalibrationResults, PlatformInfo, ScaleFactors, TestProfile};
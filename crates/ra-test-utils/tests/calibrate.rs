#![expect(clippy::print_stdout, clippy::expect_used, reason = "test uses stdout for diagnostic output")]
//! Platform calibration test.
//!
//! Run with: `cargo test --test calibrate calibrate_platform -- --ignored --nocapture`

use ra_test_utils::calibrate;

#[test]
#[ignore = "manual benchmark, run with --ignored --nocapture"]
fn calibrate_platform() {
    let profile = calibrate().expect("calibration should succeed");
    println!("\nCalibration complete!");
    println!("{profile:#?}");
}

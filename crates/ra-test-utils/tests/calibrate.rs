//! Platform calibration test.
//!
//! Run with: cargo test --test calibrate calibrate_platform -- --ignored --nocapture

use ra_test_utils::calibrate;

#[test]
#[ignore]
fn calibrate_platform() {
    let profile = calibrate().expect("calibration should succeed");
    println!("\nCalibration complete!");
    println!("{:#?}", profile);
}
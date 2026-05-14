//! Ra differential test runner.
//!
//! Connects to two PostgreSQL instances (one with Ra extension, one native)
//! and runs the test suite comparing query plans and DML results.

use ra_difftest::DiffTestRunner;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("ra_difftest=info".parse().unwrap()),
        )
        .init();

    let ra_url = std::env::var("RA_DATABASE_URL").unwrap_or_else(|_| {
        "host=localhost port=15433 user=ra_test password=ra_test dbname=ra_test".to_string()
    });
    let native_url = std::env::var("NATIVE_DATABASE_URL").unwrap_or_else(|_| {
        "host=localhost port=15432 user=ra_test password=ra_test dbname=ra_test".to_string()
    });

    tracing::info!("Connecting to Ra PG: {ra_url}");
    tracing::info!("Connecting to Native PG: {native_url}");

    let runner = match DiffTestRunner::connect(&ra_url, &native_url).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to connect: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut passed = 0u32;
    let mut failed = 0u32;

    // Run a basic smoke test suite
    let test_cases: Vec<(&str, &[&str], &str, &str)> = vec![
        (
            "insert_basic",
            &[
                "DROP TABLE IF EXISTS diff_smoke CASCADE",
                "CREATE TABLE diff_smoke (id INT PRIMARY KEY, val TEXT)",
            ],
            "INSERT INTO diff_smoke VALUES (1, 'hello'), (2, 'world')",
            "SELECT * FROM diff_smoke ORDER BY id",
        ),
        (
            "update_basic",
            &[
                "DROP TABLE IF EXISTS diff_smoke_upd CASCADE",
                "CREATE TABLE diff_smoke_upd (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_smoke_upd VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "UPDATE diff_smoke_upd SET val = val * 2 WHERE id > 1",
            "SELECT * FROM diff_smoke_upd ORDER BY id",
        ),
        (
            "delete_basic",
            &[
                "DROP TABLE IF EXISTS diff_smoke_del CASCADE",
                "CREATE TABLE diff_smoke_del (id INT PRIMARY KEY, val INT)",
                "INSERT INTO diff_smoke_del VALUES (1, 10), (2, 20), (3, 30)",
            ],
            "DELETE FROM diff_smoke_del WHERE val < 25",
            "SELECT * FROM diff_smoke_del ORDER BY id",
        ),
    ];

    for (name, setup, dml, verify) in &test_cases {
        let result = runner.compare_dml(setup, dml, verify).await;
        match result {
            ra_difftest::DiffResult::Match { rows } => {
                tracing::info!("PASS: {name} ({rows} rows matched)");
                passed += 1;
            }
            other => {
                tracing::error!("FAIL: {name} — {other:?}");
                failed += 1;
            }
        }
    }

    tracing::info!("Results: {passed} passed, {failed} failed");

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

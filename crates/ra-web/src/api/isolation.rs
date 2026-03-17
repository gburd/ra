//! POST /api/isolation/* - Isolation test endpoints.

use ra_isolation::adapter::{MockAdapter, QueryResult as IsoQueryResult};
use ra_isolation::executor::TestExecutor;
use ra_isolation::spec_parser;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

use crate::errors::{ApiResult, AppError};
use crate::rate_limit::RateGuard;

/// Request body for parsing a .spec file.
#[derive(Debug, Deserialize)]
pub struct ParseSpecRequest {
    /// Raw .spec file content.
    pub spec: String,
}

/// Response body from parsing a .spec file.
#[derive(Debug, Serialize)]
pub struct ParseSpecResponse {
    /// Whether the parse succeeded.
    pub valid: bool,
    /// Number of sessions defined.
    pub session_count: usize,
    /// Session names.
    pub sessions: Vec<SessionInfo>,
    /// Number of explicit permutations.
    pub permutation_count: usize,
    /// Number of setup statements.
    pub setup_count: usize,
    /// Number of teardown statements.
    pub teardown_count: usize,
}

/// Summary info about a parsed session.
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    /// Session name.
    pub name: String,
    /// Number of steps.
    pub step_count: usize,
    /// Step names.
    pub steps: Vec<String>,
}

/// Parse and validate a .spec file.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/isolation/parse", data = "<req>")]
pub fn parse_spec(
    req: Json<ParseSpecRequest>,
) -> ApiResult<ParseSpecResponse> {
    if req.spec.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_spec",
            ".spec content cannot be empty",
        ));
    }

    let spec =
        spec_parser::parse(&req.spec).map_err(|e| {
            AppError::bad_request("parse_error", e.to_string())
        })?;

    let sessions = spec
        .sessions
        .iter()
        .map(|s| SessionInfo {
            name: s.name.clone(),
            step_count: s.steps.len(),
            steps: s.steps.iter().map(|st| st.name.clone()).collect(),
        })
        .collect();

    Ok(Json(ParseSpecResponse {
        valid: true,
        session_count: spec.sessions.len(),
        sessions,
        permutation_count: spec.permutations.len(),
        setup_count: spec.setup.len(),
        teardown_count: spec.teardown.len(),
    }))
}

/// Request body for running an isolation test.
#[derive(Debug, Deserialize)]
pub struct RunIsolationRequest {
    /// Raw .spec file content.
    pub spec: String,
}

/// Response body from running an isolation test.
#[derive(Debug, Serialize)]
pub struct RunIsolationResponse {
    /// Whether all permutations passed.
    pub passed: bool,
    /// Number of permutations tested.
    pub permutation_count: usize,
    /// Per-permutation results.
    pub permutations: Vec<PermutationSummary>,
    /// Total events recorded.
    pub event_count: usize,
}

/// Summary of a single permutation's result.
#[derive(Debug, Serialize)]
pub struct PermutationSummary {
    /// Permutation index (0-based).
    pub index: usize,
    /// Whether this permutation passed.
    pub passed: bool,
    /// Step order used.
    pub steps: Vec<String>,
    /// Errors encountered.
    pub errors: Vec<String>,
    /// Deadlock cycles detected.
    pub deadlock_count: usize,
}

/// Run an isolation test from a .spec file.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/isolation/run", data = "<req>")]
pub fn run_isolation(
    _rate: RateGuard,
    req: Json<RunIsolationRequest>,
) -> ApiResult<RunIsolationResponse> {
    if req.spec.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_spec",
            ".spec content cannot be empty",
        ));
    }

    let spec =
        spec_parser::parse(&req.spec).map_err(|e| {
            AppError::bad_request("parse_error", e.to_string())
        })?;

    let mut executor = TestExecutor::new(spec);

    let factory: ra_isolation::executor::AdapterFactory = Box::new(
        |name: &str| -> Box<dyn ra_isolation::adapter::DatabaseAdapter> {
            Box::new(MockAdapter::new(
                name.to_owned(),
                vec![IsoQueryResult {
                    columns: vec![],
                    rows: vec![],
                    rows_affected: 0,
                }],
            ))
        },
    );

    let result = executor.run(&factory).map_err(|e| {
        AppError::internal(format!("isolation test failed: {e}"))
    })?;

    let permutations = result
        .permutation_results
        .iter()
        .map(|p| PermutationSummary {
            index: p.index,
            passed: p.passed,
            steps: p.step_descriptions.clone(),
            errors: p.errors.clone(),
            deadlock_count: p.deadlocks.len(),
        })
        .collect();

    Ok(Json(RunIsolationResponse {
        passed: result.passed,
        permutation_count: result.permutation_results.len(),
        permutations,
        event_count: result.event_log.len(),
    }))
}

//! Integration tests for the ra-isolation crate.
//!
//! These tests parse real `.spec` files and run them through
//! the executor with mock adapters to verify the full pipeline.

#![allow(clippy::panic)] // Tests may panic for early failure reporting

use ra_isolation::adapter::{
    AdapterError, DatabaseAdapter, LockDetail, LockState,
    MockAdapter, QueryResult,
};
use ra_isolation::executor::{AdapterFactory, TestExecutor};
use ra_isolation::spec_parser;

fn read_spec(name: &str) -> String {
    let path = format!(
        "{}/tests/specs/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read spec file {path}: {e}"))
}

fn mock_factory() -> AdapterFactory {
    Box::new(|name: &str| -> Box<dyn DatabaseAdapter> {
        Box::new(MockAdapter::new(name.to_owned(), vec![]))
    })
}

#[test]
fn parse_dirty_read_spec() {
    let content = read_spec("dirty-read.spec");
    let spec = spec_parser::parse(&content)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));

    assert_eq!(spec.sessions.len(), 2);
    assert_eq!(spec.sessions[0].name, "s1");
    assert_eq!(spec.sessions[1].name, "s2");
    assert_eq!(spec.sessions[0].steps.len(), 2);
    assert_eq!(spec.sessions[1].steps.len(), 1);
    assert_eq!(spec.permutations.len(), 1);
    assert_eq!(spec.setup.len(), 2);
    assert_eq!(spec.teardown.len(), 1);
}

#[test]
fn parse_phantom_read_spec() {
    let content = read_spec("phantom-read.spec");
    let spec = spec_parser::parse(&content)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));

    assert_eq!(spec.sessions.len(), 2);
    assert_eq!(spec.sessions[0].name, "s1");
    assert_eq!(spec.sessions[0].steps.len(), 2);
    assert_eq!(spec.sessions[1].name, "s2");
    assert_eq!(spec.sessions[1].steps.len(), 1);
    assert_eq!(spec.permutations.len(), 1);
}

#[test]
fn parse_deadlock_spec() {
    let content = read_spec("deadlock.spec");
    let spec = spec_parser::parse(&content)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));

    assert_eq!(spec.sessions.len(), 2);
    assert_eq!(spec.sessions[0].steps.len(), 2);
    assert_eq!(spec.sessions[1].steps.len(), 2);
    assert_eq!(spec.permutations.len(), 1);
    assert_eq!(spec.permutations[0].steps.len(), 4);
}

#[test]
fn execute_dirty_read_with_mock() {
    let content = read_spec("dirty-read.spec");
    let spec = spec_parser::parse(&content)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));
    let mut executor = TestExecutor::new(spec);
    let result = executor
        .run(&mock_factory())
        .unwrap_or_else(|e| panic!("test execution failed: {e}"));

    assert_eq!(result.permutation_results.len(), 1);
    assert!(result.passed);
}

#[test]
fn execute_phantom_read_with_mock() {
    let content = read_spec("phantom-read.spec");
    let spec = spec_parser::parse(&content)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));
    let mut executor = TestExecutor::new(spec);
    let result = executor
        .run(&mock_factory())
        .unwrap_or_else(|e| panic!("test execution failed: {e}"));

    assert_eq!(result.permutation_results.len(), 1);
    assert!(result.passed);
}

#[test]
fn execute_deadlock_with_mock() {
    let content = read_spec("deadlock.spec");
    let spec = spec_parser::parse(&content)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));
    let mut executor = TestExecutor::new(spec);
    let result = executor
        .run(&mock_factory())
        .unwrap_or_else(|e| panic!("test execution failed: {e}"));

    assert_eq!(result.permutation_results.len(), 1);
    assert!(result.passed);
}

/// Test with an adapter that simulates a deadlock error.
#[derive(Debug)]
struct DeadlockAdapter {
    call_count: usize,
    deadlock_on_call: usize,
}

impl DeadlockAdapter {
    fn new(deadlock_on_call: usize) -> Self {
        Self {
            call_count: 0,
            deadlock_on_call,
        }
    }
}

impl DatabaseAdapter for DeadlockAdapter {
    fn execute(
        &mut self,
        _sql: &str,
    ) -> Result<QueryResult, AdapterError> {
        self.call_count += 1;
        if self.call_count == self.deadlock_on_call {
            return Err(AdapterError::Deadlock);
        }
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
        })
    }

    fn lock_state(&self) -> Result<LockState, AdapterError> {
        Ok(LockState {
            held: vec![],
            waiting: vec![],
        })
    }

    fn is_blocked(&self) -> bool {
        false
    }

    fn isolation_level_name(&self) -> &'static str {
        "serializable"
    }

    fn backend_name(&self) -> &'static str {
        "test-deadlock"
    }
}

#[test]
fn detect_simulated_deadlock() {
    let input = r#"
session "s1"
{
    step "a"
    {
        UPDATE t SET v = 1 WHERE id = 1;
    }

    step "b"
    {
        UPDATE t SET v = 1 WHERE id = 2;
    }
}

session "s2"
{
    step "c"
    {
        UPDATE t SET v = 2 WHERE id = 2;
    }

    step "d"
    {
        UPDATE t SET v = 2 WHERE id = 1;
    }
}

permutation
{
    s1:a
    s2:c
    s1:b
    s2:d
}
"#;

    let spec = spec_parser::parse(input)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));

    // s2's second call (step d) will trigger a deadlock.
    // Calls are: setup=none, s1:a=1, s2:c=1, s1:b=2, s2:d=2
    // For s2, deadlock on its 2nd call.
    let factory: AdapterFactory =
        Box::new(|name: &str| -> Box<dyn DatabaseAdapter> {
            if name == "s2" {
                Box::new(DeadlockAdapter::new(2))
            } else {
                Box::new(MockAdapter::new(name.to_owned(), vec![]))
            }
        });

    let mut executor = TestExecutor::new(spec);
    let result = executor
        .run(&factory)
        .unwrap_or_else(|e| panic!("test execution failed: {e}"));

    // The executor should have detected the deadlock error
    assert!(!result.passed);
    let perm = &result.permutation_results[0];
    assert!(!perm.errors.is_empty() || !perm.deadlocks.is_empty());
}

/// Test with an adapter that reports lock state.
#[derive(Debug)]
struct LockReportingAdapter {
    name: String,
    held_locks: Vec<LockDetail>,
    waiting_locks: Vec<LockDetail>,
}

impl LockReportingAdapter {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            held_locks: vec![],
            waiting_locks: vec![],
        }
    }

    fn with_held_lock(
        mut self,
        resource: &str,
        mode: &str,
    ) -> Self {
        self.held_locks.push(LockDetail {
            resource: resource.to_owned(),
            mode: mode.to_owned(),
            granted: true,
        });
        self
    }

    fn with_waiting_lock(
        mut self,
        resource: &str,
        mode: &str,
    ) -> Self {
        self.waiting_locks.push(LockDetail {
            resource: resource.to_owned(),
            mode: mode.to_owned(),
            granted: false,
        });
        self
    }
}

impl DatabaseAdapter for LockReportingAdapter {
    fn execute(
        &mut self,
        _sql: &str,
    ) -> Result<QueryResult, AdapterError> {
        Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            rows_affected: 0,
        })
    }

    fn lock_state(&self) -> Result<LockState, AdapterError> {
        Ok(LockState {
            held: self.held_locks.clone(),
            waiting: self.waiting_locks.clone(),
        })
    }

    fn is_blocked(&self) -> bool {
        !self.waiting_locks.is_empty()
    }

    fn isolation_level_name(&self) -> &'static str {
        "read committed"
    }

    fn backend_name(&self) -> &str {
        &self.name
    }
}

#[test]
fn lock_monitoring_integration() {
    use ra_isolation::locks::LockMonitor;
    use ra_isolation::session::Session;

    let s1 = Session::new(
        "s1",
        Box::new(
            LockReportingAdapter::new("test")
                .with_held_lock("table_a", "ExclusiveLock"),
        ),
    );
    let s2 = Session::new(
        "s2",
        Box::new(
            LockReportingAdapter::new("test")
                .with_waiting_lock("table_a", "ExclusiveLock"),
        ),
    );

    let sessions = vec![s1, s2];
    let mut monitor = LockMonitor::new();
    monitor
        .refresh(&sessions)
        .unwrap_or_else(|e| panic!("lock monitor refresh failed: {e}"));

    let blocked = monitor.blocked_sessions();
    assert_eq!(blocked.len(), 1);
    assert!(blocked.contains(&"s2".to_owned()));

    let all_locks = monitor.all_locks();
    assert_eq!(all_locks.len(), 2);
}

#[test]
fn event_log_records_all_steps() {
    let input = r#"
setup
{
    CREATE TABLE t (id INT);
}

session "s1"
{
    step "a"
    {
        SELECT 1;
    }
}

permutation
{
    s1:a
}
"#;
    let spec = spec_parser::parse(input)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));
    let mut executor = TestExecutor::new(spec);
    let result = executor
        .run(&mock_factory())
        .unwrap_or_else(|e| panic!("test execution failed: {e}"));

    // Should have: PermutationStarted, SetupExecuted,
    // StepStarted, StepCompleted, PermutationCompleted
    assert!(result.event_log.len() >= 4);
}

#[test]
fn scheduler_generates_all_interleavings() {
    use ra_isolation::scheduler::Scheduler;

    let input = r#"
session "s1"
{
    step "a"
    {
        SELECT 1;
    }

    step "b"
    {
        SELECT 2;
    }
}

session "s2"
{
    step "c"
    {
        SELECT 3;
    }
}
"#;
    let spec = spec_parser::parse(input)
        .unwrap_or_else(|e| panic!("spec parsing failed: {e}"));
    let scheduler = Scheduler::from_spec(&spec);

    // 2 steps from s1, 1 from s2 -> C(3,1) = 3 interleavings
    // [a,b,c], [a,c,b], [c,a,b]
    assert_eq!(scheduler.count(), 3);

    // Verify s1 ordering is preserved in all
    for ordering in scheduler.orderings() {
        let s1_steps: Vec<&str> = ordering
            .steps
            .iter()
            .filter(|s| s.session == "s1")
            .map(|s| s.step.as_str())
            .collect();
        assert_eq!(s1_steps, vec!["a", "b"]);
    }
}

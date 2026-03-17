//! Test executor that coordinates sessions and the scheduler.
//!
//! The executor runs a complete isolation test by:
//! 1. Executing setup SQL
//! 2. Creating sessions with database adapters
//! 3. Running each step ordering from the scheduler
//! 4. Monitoring for blocking and deadlocks
//! 5. Recording events for diagnostic output
//! 6. Executing teardown SQL

use serde::{Deserialize, Serialize};

use crate::adapter::{AdapterError, DatabaseAdapter, QueryResult};
use crate::events::{TestEvent, TestEventLog};
use crate::locks::LockMonitor;
use crate::markers::Marker;
use crate::scheduler::{Scheduler, StepOrder};
use crate::session::Session;
use crate::spec_parser::{MarkerDirective, SpecFile, StepRef};

/// Executes isolation tests from a parsed `.spec` file.
pub struct TestExecutor {
    spec: SpecFile,
    scheduler: Scheduler,
    log: TestEventLog,
}

/// Result of running a complete isolation test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    /// Whether all permutations passed.
    pub passed: bool,
    /// Results for each permutation.
    pub permutation_results: Vec<PermutationResult>,
    /// The complete event log.
    pub event_log: TestEventLog,
}

/// Result of a single permutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermutationResult {
    /// Index of this permutation.
    pub index: usize,
    /// The step ordering used.
    pub step_descriptions: Vec<String>,
    /// Whether this permutation passed.
    pub passed: bool,
    /// Per-step results.
    pub step_results: Vec<StepResult>,
    /// Any deadlocks detected.
    pub deadlocks: Vec<Vec<String>>,
    /// Any errors encountered.
    pub errors: Vec<String>,
}

/// Result of executing a single step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Session name.
    pub session: String,
    /// Step name.
    pub step: String,
    /// The query result, if successful.
    pub result: Option<QueryResult>,
    /// Error message, if the step failed.
    pub error: Option<String>,
    /// Whether the step was blocked.
    pub was_blocked: bool,
}

/// Factory for creating database adapters per session.
pub type AdapterFactory =
    Box<dyn Fn(&str) -> Box<dyn DatabaseAdapter> + Send + Sync>;

impl TestExecutor {
    /// Create a new executor for the given spec.
    #[must_use]
    pub fn new(spec: SpecFile) -> Self {
        let scheduler = Scheduler::from_spec(&spec);
        Self {
            spec,
            scheduler,
            log: TestEventLog::new(),
        }
    }

    /// Run the test using the provided adapter factory.
    ///
    /// The factory is called once per session per permutation to create
    /// fresh database connections.
    ///
    /// # Errors
    ///
    /// Returns `ExecutorError` if setup, teardown, or execution fails
    /// in an unrecoverable way.
    pub fn run(
        &mut self,
        adapter_factory: &AdapterFactory,
    ) -> Result<TestResult, ExecutorError> {
        let mut permutation_results = Vec::new();
        let orderings = self.scheduler.orderings().to_vec();

        for (perm_idx, ordering) in orderings.iter().enumerate() {
            self.log
                .record(TestEvent::PermutationStarted {
                    index: perm_idx,
                    steps: ordering
                        .steps
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect(),
                });

            let perm_result = self.run_permutation(
                perm_idx,
                ordering,
                adapter_factory,
            )?;
            permutation_results.push(perm_result);

            self.log
                .record(TestEvent::PermutationCompleted {
                    index: perm_idx,
                });
        }

        let passed =
            permutation_results.iter().all(|r| r.passed);

        Ok(TestResult {
            passed,
            permutation_results,
            event_log: self.log.clone(),
        })
    }

    fn run_permutation(
        &mut self,
        index: usize,
        ordering: &StepOrder,
        adapter_factory: &AdapterFactory,
    ) -> Result<PermutationResult, ExecutorError> {
        let mut setup_session =
            Session::new("__setup__", adapter_factory("__setup__"));

        for sql in &self.spec.setup {
            self.log.record(TestEvent::SetupExecuted {
                sql: sql.clone(),
            });
            setup_session.execute_sql(sql).map_err(|e| {
                ExecutorError::SetupFailed(e.to_string())
            })?;
        }

        let mut sessions: Vec<Session> = self
            .spec
            .sessions
            .iter()
            .map(|s| {
                Session::new(
                    s.name.clone(),
                    adapter_factory(&s.name),
                )
            })
            .collect();

        let mut markers = Marker::new();
        let mut lock_monitor = LockMonitor::new();
        let mut step_results = Vec::new();
        let mut errors = Vec::new();

        for step_ref in &ordering.steps {
            let result = self.execute_step(
                step_ref,
                &mut sessions,
                &mut markers,
                &mut lock_monitor,
            );

            match result {
                Ok(sr) => {
                    if sr.error.is_some() {
                        if let Some(ref err) = sr.error {
                            errors.push(format!(
                                "{}:{}: {}",
                                sr.session, sr.step, err
                            ));
                        }
                    }
                    step_results.push(sr);
                }
                Err(e) => {
                    errors.push(format!(
                        "{}:{}: {}",
                        step_ref.session, step_ref.step, e
                    ));
                    step_results.push(StepResult {
                        session: step_ref.session.clone(),
                        step: step_ref.step.clone(),
                        result: None,
                        error: Some(e.to_string()),
                        was_blocked: false,
                    });
                }
            }
        }

        let deadlocks = lock_monitor.detect_deadlocks();
        if !deadlocks.is_empty() {
            for cycle in &deadlocks {
                self.log.record(TestEvent::DeadlockDetected {
                    sessions: cycle.clone(),
                });
            }
        }

        for sql in &self.spec.teardown {
            self.log.record(TestEvent::TeardownExecuted {
                sql: sql.clone(),
            });
            let _ = setup_session.execute_sql(sql);
        }

        let passed = errors.is_empty() && deadlocks.is_empty();

        Ok(PermutationResult {
            index,
            step_descriptions: ordering
                .steps
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            passed,
            step_results,
            deadlocks,
            errors,
        })
    }

    fn execute_step(
        &mut self,
        step_ref: &StepRef,
        sessions: &mut [Session],
        markers: &mut Marker,
        lock_monitor: &mut LockMonitor,
    ) -> Result<StepResult, ExecutorError> {
        let session_def = self
            .spec
            .sessions
            .iter()
            .find(|s| s.name == step_ref.session)
            .ok_or_else(|| {
                ExecutorError::SessionNotFound(
                    step_ref.session.clone(),
                )
            })?;

        let step_def = session_def
            .steps
            .iter()
            .find(|s| s.name == step_ref.step)
            .ok_or_else(|| {
                ExecutorError::StepNotFound(
                    step_ref.session.clone(),
                    step_ref.step.clone(),
                )
            })?;

        for directive in &step_def.markers {
            match directive {
                MarkerDirective::Wait(name) => {
                    if !markers.is_signaled(name) {
                        markers.register_waiter(
                            name,
                            &step_ref.session,
                        );
                        self.log.record(TestEvent::MarkerWaited {
                            session: step_ref.session.clone(),
                            marker: name.clone(),
                        });
                    }
                }
                MarkerDirective::Signal(_) => {}
            }
        }

        let session = sessions
            .iter_mut()
            .find(|s| s.name() == step_ref.session)
            .ok_or_else(|| {
                ExecutorError::SessionNotFound(
                    step_ref.session.clone(),
                )
            })?;

        let was_blocked = session.is_blocked();
        if was_blocked {
            self.log.record(TestEvent::SessionBlocked {
                session: step_ref.session.clone(),
                step: step_ref.step.clone(),
            });
        }

        let step_result = match session
            .execute_step(step_def, &mut self.log)
        {
            Ok(result) => StepResult {
                session: step_ref.session.clone(),
                step: step_ref.step.clone(),
                result: Some(result),
                error: None,
                was_blocked,
            },
            Err(AdapterError::Deadlock) => {
                self.log.record(TestEvent::DeadlockDetected {
                    sessions: vec![step_ref.session.clone()],
                });
                StepResult {
                    session: step_ref.session.clone(),
                    step: step_ref.step.clone(),
                    result: None,
                    error: Some("deadlock detected".into()),
                    was_blocked: true,
                }
            }
            Err(e) => StepResult {
                session: step_ref.session.clone(),
                step: step_ref.step.clone(),
                result: None,
                error: Some(e.to_string()),
                was_blocked,
            },
        };

        for directive in &step_def.markers {
            if let MarkerDirective::Signal(name) = directive {
                markers.signal(name);
                self.log.record(TestEvent::MarkerSignaled {
                    session: step_ref.session.clone(),
                    marker: name.clone(),
                });
            }
        }

        let _ = lock_monitor.refresh(sessions);

        Ok(step_result)
    }

    /// Return a reference to the event log.
    #[must_use]
    pub fn event_log(&self) -> &TestEventLog {
        &self.log
    }
}

/// Errors from the test executor.
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    /// Setup SQL failed.
    #[error("setup failed: {0}")]
    SetupFailed(String),

    /// Teardown SQL failed.
    #[error("teardown failed: {0}")]
    TeardownFailed(String),

    /// A referenced session was not found.
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// A referenced step was not found.
    #[error("step not found: {0}:{1}")]
    StepNotFound(String, String),

    /// A database adapter error.
    #[error("adapter error: {0}")]
    AdapterError(#[from] AdapterError),
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::adapter::{MockAdapter, QueryResult};
    use crate::spec_parser;

    fn mock_factory() -> AdapterFactory {
        Box::new(|name: &str| -> Box<dyn DatabaseAdapter> {
            Box::new(MockAdapter::new(
                name.to_owned(),
                vec![QueryResult {
                    columns: vec!["result".into()],
                    rows: vec![vec!["ok".into()]],
                    rows_affected: 0,
                }],
            ))
        })
    }

    fn must_parse(input: &str) -> SpecFile {
        spec_parser::parse(input).unwrap_or_else(|e| {
            panic!("parse failed: {e}");
        })
    }

    #[test]
    fn execute_simple_spec() {
        let input = r#"
setup
{
    CREATE TABLE t (id INT);
}

teardown
{
    DROP TABLE t;
}

session "s1"
{
    step "read"
    {
        SELECT * FROM t;
    }
}

permutation
{
    s1:read
}
"#;
        let spec = must_parse(input);
        let mut executor = TestExecutor::new(spec);
        let result = executor
            .run(&mock_factory())
            .unwrap_or_else(|e| panic!("execution failed: {e}"));

        assert!(result.passed);
        assert_eq!(result.permutation_results.len(), 1);
        assert!(!result.event_log.is_empty());
    }

    #[test]
    fn execute_multiple_sessions() {
        let input = r#"
session "s1"
{
    step "a"
    {
        SELECT 1;
    }
}

session "s2"
{
    step "b"
    {
        SELECT 2;
    }
}
"#;
        let spec = must_parse(input);
        let mut executor = TestExecutor::new(spec);
        let result = executor
            .run(&mock_factory())
            .unwrap_or_else(|e| panic!("execution failed: {e}"));

        assert_eq!(result.permutation_results.len(), 2);
        assert!(result.passed);
    }
}

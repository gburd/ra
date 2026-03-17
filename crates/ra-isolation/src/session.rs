//! Session management for isolation tests.
//!
//! Each session represents an independent database connection with its
//! own transaction state. Sessions are identified by name and execute
//! their steps in order, potentially interleaved with steps from
//! other sessions.

use crate::adapter::{AdapterError, DatabaseAdapter, QueryResult};
use crate::events::{TestEvent, TestEventLog};
use crate::spec_parser::StepDef;

/// The state of a session within an isolation test.
#[derive(Debug)]
pub struct Session {
    name: String,
    adapter: Box<dyn DatabaseAdapter>,
    steps_executed: usize,
}

impl Session {
    /// Create a new session with the given name and database adapter.
    pub fn new(
        name: impl Into<String>,
        adapter: Box<dyn DatabaseAdapter>,
    ) -> Self {
        Self {
            name: name.into(),
            adapter,
            steps_executed: 0,
        }
    }

    /// Return the session name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return how many steps have been executed so far.
    #[must_use]
    pub fn steps_executed(&self) -> usize {
        self.steps_executed
    }

    /// Check whether the session's adapter is currently blocked.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.adapter.is_blocked()
    }

    /// Return the backend name for this session's adapter.
    #[must_use]
    pub fn backend_name(&self) -> &str {
        self.adapter.backend_name()
    }

    /// Execute a step's SQL against this session's adapter.
    ///
    /// Records events in the provided log.
    ///
    /// # Errors
    ///
    /// Returns `AdapterError` if the SQL execution fails.
    pub fn execute_step(
        &mut self,
        step: &StepDef,
        log: &mut TestEventLog,
    ) -> Result<QueryResult, AdapterError> {
        log.record(TestEvent::StepStarted {
            session: self.name.clone(),
            step: step.name.clone(),
        });

        match self.adapter.execute(&step.sql) {
            Ok(result) => {
                self.steps_executed += 1;
                log.record(TestEvent::StepCompleted {
                    session: self.name.clone(),
                    step: step.name.clone(),
                    result: result.clone(),
                });
                Ok(result)
            }
            Err(e) => {
                log.record(TestEvent::StepFailed {
                    session: self.name.clone(),
                    step: step.name.clone(),
                    error: e.to_string(),
                });
                Err(e)
            }
        }
    }

    /// Execute raw SQL (for setup/teardown) against this session.
    ///
    /// # Errors
    ///
    /// Returns `AdapterError` if execution fails.
    pub fn execute_sql(
        &mut self,
        sql: &str,
    ) -> Result<QueryResult, AdapterError> {
        self.adapter.execute(sql)
    }

    /// Return a reference to the adapter for lock queries.
    #[must_use]
    pub fn adapter(&self) -> &dyn DatabaseAdapter {
        &*self.adapter
    }
}

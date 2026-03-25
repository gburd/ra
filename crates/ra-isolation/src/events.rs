//! Event recording for isolation test execution.
//!
//! Every significant action during a test run is captured as a
//! [`TestEvent`] and stored in a [`TestEventLog`]. This provides a
//! complete trace for debugging isolation anomalies and verifying
//! test behavior.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::adapter::QueryResult;

/// A single event recorded during test execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestEvent {
    /// A setup statement was executed.
    SetupExecuted {
        /// The SQL that was executed.
        sql: String,
    },

    /// A teardown statement was executed.
    TeardownExecuted {
        /// The SQL that was executed.
        sql: String,
    },

    /// A step began execution.
    StepStarted {
        /// Session name.
        session: String,
        /// Step name.
        step: String,
    },

    /// A step completed execution.
    StepCompleted {
        /// Session name.
        session: String,
        /// Step name.
        step: String,
        /// The query result.
        result: QueryResult,
    },

    /// A step failed with an error.
    StepFailed {
        /// Session name.
        session: String,
        /// Step name.
        step: String,
        /// Error message.
        error: String,
    },

    /// A session was detected as blocked.
    SessionBlocked {
        /// The blocked session.
        session: String,
        /// Step that is blocked.
        step: String,
    },

    /// A deadlock was detected.
    DeadlockDetected {
        /// Sessions involved in the deadlock.
        sessions: Vec<String>,
    },

    /// A marker was signaled.
    MarkerSignaled {
        /// Session that signaled.
        session: String,
        /// Marker name.
        marker: String,
    },

    /// A session waited for a marker.
    MarkerWaited {
        /// Session that waited.
        session: String,
        /// Marker name.
        marker: String,
    },

    /// A permutation began execution.
    PermutationStarted {
        /// Index of the permutation (0-based).
        index: usize,
        /// Description of step ordering.
        steps: Vec<String>,
    },

    /// A permutation completed.
    PermutationCompleted {
        /// Index of the permutation (0-based).
        index: usize,
    },
}

impl fmt::Display for TestEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SetupExecuted { sql } => {
                write!(f, "SETUP: {sql}")
            }
            Self::TeardownExecuted { sql } => {
                write!(f, "TEARDOWN: {sql}")
            }
            Self::StepStarted { session, step } => {
                write!(f, "START {session}:{step}")
            }
            Self::StepCompleted {
                session,
                step,
                result,
            } => {
                write!(f, "COMPLETE {session}:{step} -> {result}")
            }
            Self::StepFailed {
                session,
                step,
                error,
            } => {
                write!(f, "FAIL {session}:{step}: {error}")
            }
            Self::SessionBlocked { session, step } => {
                write!(f, "BLOCKED {session}:{step}")
            }
            Self::DeadlockDetected { sessions } => {
                write!(
                    f,
                    "DEADLOCK: {}",
                    sessions.join(", ")
                )
            }
            Self::MarkerSignaled { session, marker } => {
                write!(f, "SIGNAL {session}:@{marker}")
            }
            Self::MarkerWaited { session, marker } => {
                write!(f, "WAIT {session}:@{marker}")
            }
            Self::PermutationStarted { index, steps } => {
                write!(
                    f,
                    "PERMUTATION #{}: {}",
                    index,
                    steps.join(" -> ")
                )
            }
            Self::PermutationCompleted { index } => {
                write!(f, "PERMUTATION #{index} DONE")
            }
        }
    }
}

/// An ordered log of test events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestEventLog {
    events: Vec<TestEvent>,
}

impl TestEventLog {
    /// Create a new empty event log.
    #[must_use]
    pub fn new() -> Self {
        Self { events: vec![] }
    }

    /// Record a new event.
    pub fn record(&mut self, event: TestEvent) {
        tracing::debug!("{event}");
        self.events.push(event);
    }

    /// Return all recorded events.
    #[must_use]
    pub fn events(&self) -> &[TestEvent] {
        &self.events
    }

    /// Return the number of recorded events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Return whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Drain all events and return them.
    pub fn drain(&mut self) -> Vec<TestEvent> {
        std::mem::take(&mut self.events)
    }
}

impl fmt::Display for TestEventLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for event in &self.events {
            writeln!(f, "{event}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_query_result() -> QueryResult {
        QueryResult {
            columns: vec!["id".into()],
            rows: vec![vec!["1".into()]],
            rows_affected: 0,
        }
    }

    #[test]
    fn test_event_display_setup() {
        let e = TestEvent::SetupExecuted {
            sql: "CREATE TABLE t (id INT)".into(),
        };
        assert_eq!(format!("{e}"), "SETUP: CREATE TABLE t (id INT)");
    }

    #[test]
    fn test_event_display_teardown() {
        let e = TestEvent::TeardownExecuted {
            sql: "DROP TABLE t".into(),
        };
        assert_eq!(format!("{e}"), "TEARDOWN: DROP TABLE t");
    }

    #[test]
    fn test_event_display_step_started() {
        let e = TestEvent::StepStarted {
            session: "s1".into(),
            step: "read".into(),
        };
        assert_eq!(format!("{e}"), "START s1:read");
    }

    #[test]
    fn test_event_display_step_completed() {
        let e = TestEvent::StepCompleted {
            session: "s1".into(),
            step: "read".into(),
            result: sample_query_result(),
        };
        let s = format!("{e}");
        assert!(s.starts_with("COMPLETE s1:read"));
    }

    #[test]
    fn test_event_display_step_failed() {
        let e = TestEvent::StepFailed {
            session: "s1".into(),
            step: "write".into(),
            error: "lock timeout".into(),
        };
        assert_eq!(format!("{e}"), "FAIL s1:write: lock timeout");
    }

    #[test]
    fn test_event_display_session_blocked() {
        let e = TestEvent::SessionBlocked {
            session: "s1".into(),
            step: "update".into(),
        };
        assert_eq!(format!("{e}"), "BLOCKED s1:update");
    }

    #[test]
    fn test_event_display_deadlock() {
        let e = TestEvent::DeadlockDetected {
            sessions: vec!["s1".into(), "s2".into()],
        };
        assert_eq!(format!("{e}"), "DEADLOCK: s1, s2");
    }

    #[test]
    fn test_event_display_marker_signaled() {
        let e = TestEvent::MarkerSignaled {
            session: "s1".into(),
            marker: "ready".into(),
        };
        assert_eq!(format!("{e}"), "SIGNAL s1:@ready");
    }

    #[test]
    fn test_event_display_marker_waited() {
        let e = TestEvent::MarkerWaited {
            session: "s1".into(),
            marker: "ready".into(),
        };
        assert_eq!(format!("{e}"), "WAIT s1:@ready");
    }

    #[test]
    fn test_event_display_permutation_started() {
        let e = TestEvent::PermutationStarted {
            index: 0,
            steps: vec!["s1:a".into(), "s2:b".into()],
        };
        assert_eq!(format!("{e}"), "PERMUTATION #0: s1:a -> s2:b");
    }

    #[test]
    fn test_event_display_permutation_completed() {
        let e = TestEvent::PermutationCompleted { index: 2 };
        assert_eq!(format!("{e}"), "PERMUTATION #2 DONE");
    }

    #[test]
    fn test_event_log_new_is_empty() {
        let log = TestEventLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert!(log.events().is_empty());
    }

    #[test]
    fn test_event_log_record_and_retrieve() {
        let mut log = TestEventLog::new();
        log.record(TestEvent::SetupExecuted {
            sql: "SELECT 1".into(),
        });
        assert!(!log.is_empty());
        assert_eq!(log.len(), 1);
        assert_eq!(log.events().len(), 1);
    }

    #[test]
    fn test_event_log_drain() {
        let mut log = TestEventLog::new();
        log.record(TestEvent::SetupExecuted {
            sql: "SELECT 1".into(),
        });
        log.record(TestEvent::TeardownExecuted {
            sql: "DROP TABLE t".into(),
        });
        let events = log.drain();
        assert_eq!(events.len(), 2);
        assert!(log.is_empty());
    }

    #[test]
    fn test_event_log_display() {
        let mut log = TestEventLog::new();
        log.record(TestEvent::SetupExecuted {
            sql: "SELECT 1".into(),
        });
        let display = format!("{log}");
        assert!(display.contains("SETUP: SELECT 1"));
    }

    #[test]
    fn test_event_log_default() {
        let log = TestEventLog::default();
        assert!(log.is_empty());
    }
}

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

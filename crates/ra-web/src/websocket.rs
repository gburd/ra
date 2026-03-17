//! WebSocket endpoint for real-time isolation test updates.
//!
//! Clients connect to `/ws/isolation` and send a `.spec` file
//! as text. The server streams back JSON events as each
//! permutation and step completes.

use rocket::futures::{SinkExt, StreamExt};
use rocket_ws::{Channel, Message, WebSocket};
use serde::Serialize;

use ra_isolation::adapter::{
    MockAdapter, QueryResult as IsoQueryResult,
};
use ra_isolation::executor::{TestExecutor, TestResult};
use ra_isolation::spec_parser::{self, SpecFile};

/// A real-time event streamed over the WebSocket.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum WsEvent {
    /// Parsing started.
    #[serde(rename = "parse_start")]
    ParseStart,
    /// Parsing succeeded.
    #[serde(rename = "parsed")]
    Parsed {
        session_count: usize,
        permutation_count: usize,
    },
    /// A permutation is starting.
    #[serde(rename = "permutation_start")]
    PermutationStart { index: usize, total: usize },
    /// A permutation completed.
    #[serde(rename = "permutation_done")]
    PermutationDone {
        index: usize,
        passed: bool,
        errors: Vec<String>,
    },
    /// All permutations finished.
    #[serde(rename = "done")]
    Done { passed: bool, total: usize },
    /// An error occurred.
    #[serde(rename = "error")]
    Error { message: String },
}

fn to_msg(event: &WsEvent) -> Option<Message> {
    serde_json::to_string(event).ok().map(Message::Text)
}

/// Run the isolation executor synchronously.
///
/// This runs on a blocking thread because `AdapterFactory`
/// is not `Send`.
fn run_isolation(spec: SpecFile) -> Result<TestResult, String> {
    let mut executor = TestExecutor::new(spec);
    let factory: ra_isolation::executor::AdapterFactory =
        Box::new(
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
    executor
        .run(&factory)
        .map_err(|e| format!("execution failed: {e}"))
}

/// WebSocket handler for streaming isolation test results.
#[allow(clippy::needless_pass_by_value)]
#[rocket::get("/ws/isolation")]
pub fn isolation_ws(ws: WebSocket) -> Channel<'static> {
    ws.channel(move |mut stream| {
        Box::pin(async move {
            // Wait for the client to send the .spec content.
            let spec_text = loop {
                match stream.next().await {
                    Some(Ok(Message::Text(text))) => {
                        break text;
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        return Ok(());
                    }
                    _ => {},
                }
            };

            // Send parse_start event.
            if let Some(msg) = to_msg(&WsEvent::ParseStart) {
                let _ = stream.send(msg).await;
            }

            // Parse the spec (synchronous, cheap).
            let spec = match spec_parser::parse(&spec_text) {
                Ok(s) => s,
                Err(e) => {
                    if let Some(msg) =
                        to_msg(&WsEvent::Error {
                            message: e.to_string(),
                        })
                    {
                        let _ = stream.send(msg).await;
                    }
                    return Ok(());
                }
            };

            let permutation_count =
                if spec.permutations.is_empty() {
                    0
                } else {
                    spec.permutations.len()
                };

            if let Some(msg) = to_msg(&WsEvent::Parsed {
                session_count: spec.sessions.len(),
                permutation_count,
            }) {
                let _ = stream.send(msg).await;
            }

            // Run the executor on a blocking thread to avoid
            // Send issues with AdapterFactory.
            let result =
                tokio::task::spawn_blocking(move || {
                    run_isolation(spec)
                })
                .await
                .map_err(|e| {
                    rocket_ws::result::Error::Io(
                        std::io::Error::other(e.to_string()),
                    )
                })?;

            let result = match result {
                Ok(r) => r,
                Err(msg) => {
                    if let Some(m) =
                        to_msg(&WsEvent::Error { message: msg })
                    {
                        let _ = stream.send(m).await;
                    }
                    return Ok(());
                }
            };

            // Stream per-permutation results.
            let total = result.permutation_results.len();
            for p in &result.permutation_results {
                if let Some(msg) =
                    to_msg(&WsEvent::PermutationStart {
                        index: p.index,
                        total,
                    })
                {
                    let _ = stream.send(msg).await;
                }

                if let Some(msg) =
                    to_msg(&WsEvent::PermutationDone {
                        index: p.index,
                        passed: p.passed,
                        errors: p.errors.clone(),
                    })
                {
                    let _ = stream.send(msg).await;
                }
            }

            if let Some(msg) = to_msg(&WsEvent::Done {
                passed: result.passed,
                total,
            }) {
                let _ = stream.send(msg).await;
            }

            Ok(())
        })
    })
}

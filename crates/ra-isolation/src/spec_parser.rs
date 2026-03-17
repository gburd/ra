//! Parser for PostgreSQL-format `.spec` files.
//!
//! A `.spec` file defines an isolation test with the following sections:
//!
//! - `setup` - SQL executed once before all sessions start
//! - `teardown` - SQL executed after all sessions complete
//! - `session "<name>"` - A named session containing ordered steps
//! - `step "<name>"` - A named step within a session containing SQL
//! - `permutation` - An explicit step ordering across sessions
//!
//! Steps may contain marker directives (`-- @marker <name>` and
//! `-- @wait <name>`) for synchronization between sessions.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A fully parsed `.spec` file representing an isolation test.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpecFile {
    /// SQL to execute before any sessions begin.
    pub setup: Vec<String>,
    /// SQL to execute after all sessions complete.
    pub teardown: Vec<String>,
    /// Named sessions, each with an ordered list of steps.
    pub sessions: Vec<SessionDef>,
    /// Explicit step orderings. If empty, all permutations are tested.
    pub permutations: Vec<Permutation>,
}

/// A named session definition within a `.spec` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionDef {
    /// Session name (must be unique within the spec file).
    pub name: String,
    /// Ordered steps for this session.
    pub steps: Vec<StepDef>,
}

/// A named step containing SQL and optional markers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepDef {
    /// Step name (must be unique within the session).
    pub name: String,
    /// SQL statements to execute in this step.
    pub sql: String,
    /// Marker directives found within this step's SQL.
    pub markers: Vec<MarkerDirective>,
}

/// A synchronization marker directive parsed from step SQL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarkerDirective {
    /// Signal that this point has been reached.
    Signal(String),
    /// Wait for another session to reach a marker.
    Wait(String),
}

/// An explicit permutation specifying step execution order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Permutation {
    /// Ordered list of step references as `session_name:step_name`.
    pub steps: Vec<StepRef>,
}

/// A reference to a step within a session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepRef {
    /// The session that owns the step.
    pub session: String,
    /// The step name.
    pub step: String,
}

impl fmt::Display for StepRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.session, self.step)
    }
}

/// Errors that can occur during `.spec` file parsing.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Unexpected token at the given line.
    #[error("line {line}: unexpected token: {message}")]
    UnexpectedToken {
        /// Line number (1-based).
        line: usize,
        /// Description of the unexpected token.
        message: String,
    },

    /// A required section or directive is missing.
    #[error("line {line}: {message}")]
    MissingSection {
        /// Line number (1-based).
        line: usize,
        /// Description of the missing section.
        message: String,
    },

    /// Duplicate name encountered.
    #[error("line {line}: duplicate name: {name}")]
    DuplicateName {
        /// Line number (1-based).
        line: usize,
        /// The duplicated name.
        name: String,
    },

    /// Invalid step reference in a permutation.
    #[error(
        "line {line}: invalid step reference '{reference}': {reason}"
    )]
    InvalidStepRef {
        /// Line number (1-based).
        line: usize,
        /// The invalid reference string.
        reference: String,
        /// Why it's invalid.
        reason: String,
    },
}

/// Parse a `.spec` file from its text content.
///
/// # Errors
///
/// Returns `ParseError` if the content does not conform to the expected
/// `.spec` file format.
pub fn parse(input: &str) -> Result<SpecFile, ParseError> {
    Parser::new(input).parse()
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            lines: input.lines().collect(),
            pos: 0,
        }
    }

    fn line_number(&self) -> usize {
        self.pos + 1
    }

    fn current_line(&self) -> Option<&'a str> {
        self.lines.get(self.pos).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_blank_and_comments(&mut self) {
        while let Some(line) = self.current_line() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn parse(&mut self) -> Result<SpecFile, ParseError> {
        let mut setup = Vec::new();
        let mut teardown = Vec::new();
        let mut sessions = Vec::new();
        let mut permutations = Vec::new();

        loop {
            self.skip_blank_and_comments();
            let Some(line) = self.current_line() else {
                break;
            };
            let trimmed = line.trim();

            if trimmed == "setup" {
                self.advance();
                setup = self.parse_sql_block()?;
            } else if trimmed == "teardown" {
                self.advance();
                teardown = self.parse_sql_block()?;
            } else if let Some(name) = strip_directive(trimmed, "session")
            {
                self.advance();
                let session = self.parse_session(name)?;
                if sessions
                    .iter()
                    .any(|s: &SessionDef| s.name == session.name)
                {
                    return Err(ParseError::DuplicateName {
                        line: self.line_number(),
                        name: session.name,
                    });
                }
                sessions.push(session);
            } else if trimmed == "permutation" {
                self.advance();
                let perm = self.parse_permutation(&sessions)?;
                permutations.push(perm);
            } else {
                return Err(ParseError::UnexpectedToken {
                    line: self.line_number(),
                    message: format!("expected setup, teardown, session, or permutation; got: {trimmed}"),
                });
            }
        }

        Ok(SpecFile {
            setup,
            teardown,
            sessions,
            permutations,
        })
    }

    fn parse_sql_block(&mut self) -> Result<Vec<String>, ParseError> {
        let open_line = self.line_number();
        self.skip_blank_and_comments();

        let Some(line) = self.current_line() else {
            return Err(ParseError::MissingSection {
                line: open_line,
                message: "expected '{' to open SQL block".into(),
            });
        };

        if line.trim() != "{" {
            return Err(ParseError::UnexpectedToken {
                line: self.line_number(),
                message: format!(
                    "expected '{{' to open SQL block, got: {}",
                    line.trim()
                ),
            });
        }
        self.advance();

        let mut statements = Vec::new();
        let mut current = String::new();

        loop {
            let Some(line) = self.current_line() else {
                return Err(ParseError::MissingSection {
                    line: open_line,
                    message: "unclosed SQL block (missing '}')".into(),
                });
            };

            if line.trim() == "}" {
                self.advance();
                let trimmed = current.trim().to_owned();
                if !trimmed.is_empty() {
                    statements.push(trimmed);
                }
                return Ok(statements);
            }

            if line.trim().ends_with(';') {
                current.push_str(line.trim());
                current.push('\n');
                let trimmed = current.trim().to_owned();
                if !trimmed.is_empty() {
                    statements.push(trimmed);
                }
                current = String::new();
            } else {
                current.push_str(line.trim());
                current.push('\n');
            }

            self.advance();
        }
    }

    fn parse_session(
        &mut self,
        name: String,
    ) -> Result<SessionDef, ParseError> {
        let open_line = self.line_number();
        self.skip_blank_and_comments();

        let Some(line) = self.current_line() else {
            return Err(ParseError::MissingSection {
                line: open_line,
                message: "expected '{' to open session block".into(),
            });
        };

        if line.trim() != "{" {
            return Err(ParseError::UnexpectedToken {
                line: self.line_number(),
                message: format!(
                    "expected '{{' to open session block, got: {}",
                    line.trim()
                ),
            });
        }
        self.advance();

        let mut steps = Vec::new();
        let mut seen_steps = std::collections::HashSet::new();

        loop {
            self.skip_blank_and_comments();
            let Some(line) = self.current_line() else {
                return Err(ParseError::MissingSection {
                    line: open_line,
                    message: "unclosed session block (missing '}')"
                        .into(),
                });
            };
            let trimmed = line.trim();

            if trimmed == "}" {
                self.advance();
                return Ok(SessionDef { name, steps });
            }

            if let Some(step_name) = strip_directive(trimmed, "step") {
                if !seen_steps.insert(step_name.clone()) {
                    return Err(ParseError::DuplicateName {
                        line: self.line_number(),
                        name: step_name,
                    });
                }
                self.advance();
                let step = self.parse_step(step_name)?;
                steps.push(step);
            } else {
                return Err(ParseError::UnexpectedToken {
                    line: self.line_number(),
                    message: format!(
                        "expected step or '}}'; got: {trimmed}"
                    ),
                });
            }
        }
    }

    fn parse_step(
        &mut self,
        name: String,
    ) -> Result<StepDef, ParseError> {
        let open_line = self.line_number();
        self.skip_blank_and_comments();

        let Some(line) = self.current_line() else {
            return Err(ParseError::MissingSection {
                line: open_line,
                message: "expected '{' to open step block".into(),
            });
        };

        if line.trim() != "{" {
            return Err(ParseError::UnexpectedToken {
                line: self.line_number(),
                message: format!(
                    "expected '{{' to open step block, got: {}",
                    line.trim()
                ),
            });
        }
        self.advance();

        let mut sql_lines = Vec::new();
        let mut markers = Vec::new();

        loop {
            let Some(line) = self.current_line() else {
                return Err(ParseError::MissingSection {
                    line: open_line,
                    message: "unclosed step block (missing '}')".into(),
                });
            };

            if line.trim() == "}" {
                self.advance();
                return Ok(StepDef {
                    name,
                    sql: sql_lines.join("\n").trim().to_owned(),
                    markers,
                });
            }

            let trimmed = line.trim();
            if let Some(marker_name) =
                trimmed.strip_prefix("-- @marker ")
            {
                markers.push(MarkerDirective::Signal(
                    marker_name.trim().to_owned(),
                ));
            } else if let Some(wait_name) =
                trimmed.strip_prefix("-- @wait ")
            {
                markers.push(MarkerDirective::Wait(
                    wait_name.trim().to_owned(),
                ));
            }
            sql_lines.push(trimmed.to_owned());
            self.advance();
        }
    }

    fn parse_permutation(
        &mut self,
        sessions: &[SessionDef],
    ) -> Result<Permutation, ParseError> {
        let open_line = self.line_number();
        self.skip_blank_and_comments();

        let Some(line) = self.current_line() else {
            return Err(ParseError::MissingSection {
                line: open_line,
                message: "expected '{' to open permutation block".into(),
            });
        };

        if line.trim() != "{" {
            return Err(ParseError::UnexpectedToken {
                line: self.line_number(),
                message: format!(
                    "expected '{{' to open permutation block, got: {}",
                    line.trim()
                ),
            });
        }
        self.advance();

        let mut steps = Vec::new();

        loop {
            self.skip_blank_and_comments();
            let Some(line) = self.current_line() else {
                return Err(ParseError::MissingSection {
                    line: open_line,
                    message: "unclosed permutation block (missing '}')"
                        .into(),
                });
            };
            let trimmed = line.trim();

            if trimmed == "}" {
                self.advance();
                return Ok(Permutation { steps });
            }

            let step_ref =
                self.parse_step_ref(trimmed, sessions)?;
            steps.push(step_ref);
            self.advance();
        }
    }

    fn parse_step_ref(
        &self,
        text: &str,
        sessions: &[SessionDef],
    ) -> Result<StepRef, ParseError> {
        let Some((session, step)) = text.split_once(':') else {
            return Err(ParseError::InvalidStepRef {
                line: self.line_number(),
                reference: text.to_owned(),
                reason: "expected 'session:step' format".into(),
            });
        };

        let session = session.trim().to_owned();
        let step = step.trim().to_owned();

        let Some(session_def) =
            sessions.iter().find(|s| s.name == session)
        else {
            return Err(ParseError::InvalidStepRef {
                line: self.line_number(),
                reference: text.to_owned(),
                reason: format!("session '{session}' not defined"),
            });
        };

        if !session_def.steps.iter().any(|s| s.name == step) {
            return Err(ParseError::InvalidStepRef {
                line: self.line_number(),
                reference: text.to_owned(),
                reason: format!(
                    "step '{step}' not defined in session '{session}'"
                ),
            });
        }

        Ok(StepRef { session, step })
    }
}

/// Strip a directive keyword and its quoted argument.
///
/// For example, `strip_directive("session \"s1\"", "session")`
/// returns `Some("s1")`.
fn strip_directive(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?.trim_start();
    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"')?;
        Some(inner[..end].to_owned())
    } else {
        Some(rest.to_owned())
    }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn must_parse(input: &str) -> SpecFile {
        parse(input).unwrap_or_else(|e| {
            panic!("parse failed: {e}");
        })
    }

    #[test]
    fn parse_minimal_spec() {
        let input = r#"
setup
{
    CREATE TABLE t (id INT, val INT);
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

session "s2"
{
    step "write"
    {
        INSERT INTO t VALUES (1, 100);
    }
}
"#;
        let spec = must_parse(input);
        assert_eq!(spec.sessions.len(), 2);
        assert_eq!(spec.sessions[0].name, "s1");
        assert_eq!(spec.sessions[0].steps[0].name, "read");
        assert_eq!(spec.sessions[1].name, "s2");
        assert_eq!(spec.sessions[1].steps[0].name, "write");
        assert!(!spec.setup.is_empty());
        assert!(!spec.teardown.is_empty());
    }

    #[test]
    fn parse_permutation() {
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

session "s2"
{
    step "b"
    {
        SELECT 2;
    }
}

permutation
{
    s1:a
    s2:b
}

permutation
{
    s2:b
    s1:a
}
"#;
        let spec = must_parse(input);
        assert_eq!(spec.permutations.len(), 2);
        assert_eq!(spec.permutations[0].steps[0].session, "s1");
        assert_eq!(spec.permutations[0].steps[0].step, "a");
        assert_eq!(spec.permutations[1].steps[0].session, "s2");
    }

    #[test]
    fn parse_markers() {
        let input = r#"
session "s1"
{
    step "lock"
    {
        BEGIN;
        UPDATE t SET val = 1 WHERE id = 1;
        -- @marker locked
        COMMIT;
    }
}

session "s2"
{
    step "wait_and_read"
    {
        -- @wait locked
        SELECT val FROM t WHERE id = 1;
    }
}
"#;
        let spec = must_parse(input);
        let s1_step = &spec.sessions[0].steps[0];
        assert_eq!(s1_step.markers.len(), 1);
        assert_eq!(
            s1_step.markers[0],
            MarkerDirective::Signal("locked".into())
        );
        let s2_step = &spec.sessions[1].steps[0];
        assert_eq!(
            s2_step.markers[0],
            MarkerDirective::Wait("locked".into())
        );
    }

    #[test]
    fn error_on_duplicate_session() {
        let input = r#"
session "s1"
{
    step "a"
    {
        SELECT 1;
    }
}

session "s1"
{
    step "b"
    {
        SELECT 2;
    }
}
"#;
        let err = parse(input).unwrap_err();
        assert!(
            err.to_string().contains("duplicate"),
            "expected duplicate error, got: {err}"
        );
    }

    #[test]
    fn error_on_invalid_step_ref() {
        let input = r#"
session "s1"
{
    step "a"
    {
        SELECT 1;
    }
}

permutation
{
    s1:nonexistent
}
"#;
        let err = parse(input).unwrap_err();
        assert!(
            err.to_string().contains("not defined"),
            "expected step ref error, got: {err}"
        );
    }

    #[test]
    fn error_on_unclosed_block() {
        let input = r"
setup
{
    CREATE TABLE t (id INT);
";
        let err = parse(input).unwrap_err();
        assert!(
            err.to_string().contains("unclosed")
                || err.to_string().contains("missing '}'"),
            "expected unclosed block error, got: {err}"
        );
    }
}

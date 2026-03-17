//! Step scheduler with permutation support.
//!
//! The scheduler determines the order in which steps from different
//! sessions are executed. If the `.spec` file defines explicit
//! permutations, those orderings are used. Otherwise, the scheduler
//! generates all possible interleavings of steps that preserve
//! per-session ordering.

use crate::spec_parser::{Permutation, SessionDef, SpecFile, StepRef};

/// Controls the ordering of steps across sessions.
#[derive(Debug)]
pub struct Scheduler {
    orderings: Vec<StepOrder>,
}

/// A single ordering of steps to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOrder {
    /// The ordered sequence of step references.
    pub steps: Vec<StepRef>,
}

impl Scheduler {
    /// Build a scheduler from a parsed spec file.
    ///
    /// If the spec defines permutations, use those. Otherwise,
    /// generate all valid interleavings.
    #[must_use]
    pub fn from_spec(spec: &SpecFile) -> Self {
        if spec.permutations.is_empty() {
            let orderings = generate_all_orderings(&spec.sessions);
            Self { orderings }
        } else {
            let orderings = spec
                .permutations
                .iter()
                .map(|p| StepOrder {
                    steps: p.steps.clone(),
                })
                .collect();
            Self { orderings }
        }
    }

    /// Build a scheduler from explicit permutations.
    #[must_use]
    pub fn from_permutations(perms: &[Permutation]) -> Self {
        let orderings = perms
            .iter()
            .map(|p| StepOrder {
                steps: p.steps.clone(),
            })
            .collect();
        Self { orderings }
    }

    /// Return all orderings to execute.
    #[must_use]
    pub fn orderings(&self) -> &[StepOrder] {
        &self.orderings
    }

    /// Return the number of orderings.
    #[must_use]
    pub fn count(&self) -> usize {
        self.orderings.len()
    }
}

/// Generate all valid interleavings of steps across sessions.
///
/// Each session's steps must maintain their relative order, but steps
/// from different sessions can be arbitrarily interleaved.
fn generate_all_orderings(sessions: &[SessionDef]) -> Vec<StepOrder> {
    let step_lists: Vec<Vec<StepRef>> = sessions
        .iter()
        .map(|s| {
            s.steps
                .iter()
                .map(|step| StepRef {
                    session: s.name.clone(),
                    step: step.name.clone(),
                })
                .collect()
        })
        .collect();

    let indices: Vec<usize> = vec![0; step_lists.len()];
    let mut results = Vec::new();
    let mut current = Vec::new();

    interleave(&step_lists, &indices, &mut current, &mut results);
    results
}

fn interleave(
    step_lists: &[Vec<StepRef>],
    indices: &[usize],
    current: &mut Vec<StepRef>,
    results: &mut Vec<StepOrder>,
) {
    let total_remaining: usize = step_lists
        .iter()
        .zip(indices.iter())
        .map(|(list, &idx)| list.len() - idx)
        .sum();

    if total_remaining == 0 {
        results.push(StepOrder {
            steps: current.clone(),
        });
        return;
    }

    for (i, (list, &idx)) in
        step_lists.iter().zip(indices.iter()).enumerate()
    {
        if idx < list.len() {
            current.push(list[idx].clone());
            let mut new_indices = indices.to_vec();
            new_indices[i] += 1;
            interleave(
                step_lists,
                &new_indices,
                current,
                results,
            );
            current.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec_parser::{SessionDef, StepDef};

    fn make_session(
        name: &str,
        step_names: &[&str],
    ) -> SessionDef {
        SessionDef {
            name: name.to_owned(),
            steps: step_names
                .iter()
                .map(|s| StepDef {
                    name: (*s).to_owned(),
                    sql: String::new(),
                    markers: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn two_sessions_one_step_each() {
        let sessions = vec![
            make_session("s1", &["a"]),
            make_session("s2", &["b"]),
        ];
        let orderings = generate_all_orderings(&sessions);
        // 2 sessions, 1 step each -> 2 orderings: [a,b] and [b,a]
        assert_eq!(orderings.len(), 2);
    }

    #[test]
    fn single_session_preserves_order() {
        let sessions = vec![make_session("s1", &["a", "b", "c"])];
        let orderings = generate_all_orderings(&sessions);
        assert_eq!(orderings.len(), 1);
        assert_eq!(orderings[0].steps[0].step, "a");
        assert_eq!(orderings[0].steps[1].step, "b");
        assert_eq!(orderings[0].steps[2].step, "c");
    }

    #[test]
    fn two_sessions_two_steps_each() {
        let sessions = vec![
            make_session("s1", &["a", "b"]),
            make_session("s2", &["c", "d"]),
        ];
        let orderings = generate_all_orderings(&sessions);
        // C(4,2) = 6 valid interleavings
        assert_eq!(orderings.len(), 6);

        // Verify per-session ordering is preserved
        for ordering in &orderings {
            let s1_steps: Vec<&str> = ordering
                .steps
                .iter()
                .filter(|s| s.session == "s1")
                .map(|s| s.step.as_str())
                .collect();
            assert_eq!(s1_steps, vec!["a", "b"]);

            let s2_steps: Vec<&str> = ordering
                .steps
                .iter()
                .filter(|s| s.session == "s2")
                .map(|s| s.step.as_str())
                .collect();
            assert_eq!(s2_steps, vec!["c", "d"]);
        }
    }

    #[test]
    fn from_spec_with_permutations() {
        let spec = SpecFile {
            setup: vec![],
            teardown: vec![],
            sessions: vec![
                make_session("s1", &["a"]),
                make_session("s2", &["b"]),
            ],
            permutations: vec![Permutation {
                steps: vec![
                    StepRef {
                        session: "s1".into(),
                        step: "a".into(),
                    },
                    StepRef {
                        session: "s2".into(),
                        step: "b".into(),
                    },
                ],
            }],
        };

        let scheduler = Scheduler::from_spec(&spec);
        assert_eq!(scheduler.count(), 1);
    }

    #[test]
    fn from_spec_without_permutations() {
        let spec = SpecFile {
            setup: vec![],
            teardown: vec![],
            sessions: vec![
                make_session("s1", &["a"]),
                make_session("s2", &["b"]),
            ],
            permutations: vec![],
        };

        let scheduler = Scheduler::from_spec(&spec);
        assert_eq!(scheduler.count(), 2);
    }
}

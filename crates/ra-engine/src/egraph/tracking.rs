use ra_core::algebra::RelExpr;

/// Detailed tracking of rule applications during optimization.
#[derive(Debug, Clone)]
pub struct RuleTrackingResult {
    /// Rules that successfully modified the e-graph.
    pub applied: Vec<RuleApplication>,
    /// Rules that were tried but didn't match or add nodes.
    pub evaluated: Vec<RuleEvaluation>,
    /// All rules available in the system.
    pub available: Vec<String>,
    /// Intermediate optimization steps (only populated in verbose mode).
    pub intermediate_steps: Option<Vec<IntermediateStep>>,
}

/// A single step in the optimization process showing plan transformation.
#[derive(Debug, Clone)]
pub struct IntermediateStep {
    /// Step number in the optimization sequence.
    pub step_number: usize,
    /// Name of the rule that was applied.
    pub rule_name: String,
    /// Explanation of why this rule was chosen.
    pub reason: String,
    /// The plan before applying the rule.
    pub plan_before: RelExpr,
    /// The plan after applying the rule.
    pub plan_after: RelExpr,
    /// Cost improvement from this step.
    pub cost_improvement: Option<f64>,
}

/// A rule that successfully applied and modified the e-graph.
#[derive(Debug, Clone)]
pub struct RuleApplication {
    /// Name of the rule.
    pub name: String,
    /// Number of times this rule fired.
    pub fired_count: usize,
    /// E-graph nodes added by this rule.
    pub nodes_added: usize,
    /// Cost improvement, if measurable.
    pub cost_improvement: Option<f64>,
}

/// A rule that was evaluated but didn't contribute to the e-graph.
#[derive(Debug, Clone)]
pub struct RuleEvaluation {
    /// Name of the rule.
    pub name: String,
    /// Number of times this rule was tried.
    pub tried_count: usize,
    /// Why the rule was rejected.
    pub rejection_reason: String,
}

/// Build a simple tracking result.
///
/// Since egg doesn't expose per-rule application statistics, we track
/// optimization at a high level: whether rules made changes, total nodes
/// added, and cost improvement.
#[allow(dead_code)]
pub(crate) fn build_simple_tracking(
    available_rules: Vec<String>,
    total_nodes_added: usize,
    iterations_with_changes: usize,
    initial_cost: f64,
    final_cost: f64,
) -> RuleTrackingResult {
    let cost_improvement = if final_cost < initial_cost {
        initial_cost - final_cost
    } else {
        0.0
    };

    let applied = if total_nodes_added > 0 {
        vec![RuleApplication {
            name: format!(
                "Aggregate: {} iteration(s) with rule applications",
                iterations_with_changes
            ),
            fired_count: iterations_with_changes,
            nodes_added: total_nodes_added,
            cost_improvement: if cost_improvement > 0.0 {
                Some(cost_improvement)
            } else {
                None
            },
        }]
    } else {
        Vec::new()
    };

    let evaluated = if total_nodes_added == 0 && !available_rules.is_empty() {
        vec![RuleEvaluation {
            name: format!(
                "Aggregate: {} rule(s) available but none applied",
                available_rules.len()
            ),
            tried_count: available_rules.len(),
            rejection_reason: "no pattern matched or no improvement".to_string(),
        }]
    } else {
        Vec::new()
    };

    RuleTrackingResult {
        applied,
        evaluated,
        available: available_rules,
        intermediate_steps: None,
    }
}

/// Build a detailed tracking result from per-rule applications.
///
/// This function takes the collected rule applications and creates
/// a tracking result with per-rule information.
pub(crate) fn build_detailed_tracking(
    available_rules: Vec<String>,
    rule_applications: Vec<RuleApplication>,
) -> RuleTrackingResult {
    build_detailed_tracking_with_steps(available_rules, rule_applications, None)
}

pub(crate) fn build_detailed_tracking_with_steps(
    available_rules: Vec<String>,
    rule_applications: Vec<RuleApplication>,
    intermediate_steps: Option<Vec<IntermediateStep>>,
) -> RuleTrackingResult {
    let evaluated = if rule_applications.is_empty() && !available_rules.is_empty() {
        vec![RuleEvaluation {
            name: format!(
                "{} rule(s) available but none applied",
                available_rules.len()
            ),
            tried_count: available_rules.len(),
            rejection_reason: "no pattern matched or no improvement".to_string(),
        }]
    } else {
        Vec::new()
    };

    RuleTrackingResult {
        applied: rule_applications,
        evaluated,
        available: available_rules,
        intermediate_steps,
    }
}

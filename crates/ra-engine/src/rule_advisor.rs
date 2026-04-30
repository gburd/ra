//! Rule Advisor: intelligent rule filtering, ranking, and learning.
//!
//! The rule advisor is a three-stage pipeline that eliminates irrelevant
//! rewrite rules before e-graph exploration:
//!
//! 1. **Context elimination** — filters by database engine and hardware
//!    capabilities (once per optimizer instance).
//! 2. **Query-shape elimination** — filters by structural and content-type
//!    features detected in the query (per query).
//! 3. **Learned ranking** — reorders surviving rules by historical
//!    effectiveness (per query shape bucket).
//!
//! The advisor integrates with [`ResourceBudget`] to support workload-specific
//! rule selection strategies (OLTP vs OLAP vs research), adaptive learning that
//! dynamically adjusts rule sets based on observed success rates, and rule
//! consolidation for long-term rule set optimization.
//!
//! ## Rule Applicability
//!
//! The [`RuleApplicability`] trait enables rules to self-describe their
//! applicability conditions. Rule groups implement this to declare which
//! query features they require, creating a separate applicability dataflow
//! that reacts to system fact changes without re-evaluating the full
//! pipeline.
//!
//! ## Adaptive Learning
//!
//! The advisor maintains per-rule usage statistics ([`RuleUsageStats`])
//! that track hit/miss rates, promotion/demotion events, and effectiveness
//! trends. Rules are promoted when their success rate exceeds the
//! promotion threshold and demoted when they fall below the demotion
//! threshold, enabling JIT-like adaptive optimization.
//!
//! [`ResourceBudget`]: crate::resource_budget::ResourceBudget

use std::collections::HashMap;

use egg::Rewrite;
use tracing::{debug, info};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::lazy_rules::LazyQueryPattern;
use crate::query_features::QueryFeatureSet;
use crate::resource_budget::{ResourceBudget, RuleSelectionBehavior};
use crate::rewrite::{AnnotatedRuleGroup, RuleAnnotation};
use crate::rule_consolidation::RuleConsolidator;
use crate::rule_knowledge::{RuleKnowledge, ShapeKeyBucket};
use ra_core::algebra::RelExpr;

/// Self-describing rule applicability.
///
/// Rule groups implement this trait to declare their applicability
/// conditions, enabling the advisor to build a separate applicability
/// dataflow that reacts to system fact changes without re-evaluating
/// the full three-stage pipeline.
///
/// The trait separates "can this rule ever apply?" (static) from
/// "should we try this rule now?" (dynamic), allowing the advisor
/// to cache static decisions and only re-evaluate dynamic conditions
/// when facts change.
pub trait RuleApplicability {
    /// Static features required by this rule group.
    ///
    /// The rule group is skipped entirely if the query does not
    /// exhibit any of these features. Return `QueryFeatureSet::UNIVERSAL`
    /// if the rule applies regardless of query structure.
    fn required_features(&self) -> QueryFeatureSet;

    /// Database engines this rule targets.
    ///
    /// Return an empty slice for universal (engine-independent) rules.
    fn target_databases(&self) -> &[&str];

    /// Whether this rule is applicable given the current query features
    /// and the active resource budget behavior.
    ///
    /// This is the dynamic applicability check. The default
    /// implementation checks feature overlap only. Override for
    /// rules with more complex preconditions (e.g., fact-dependent
    /// rules that check table statistics or hardware capabilities).
    fn is_applicable(
        &self,
        query_features: QueryFeatureSet,
        _behavior: &RuleSelectionBehavior,
    ) -> bool {
        let required = self.required_features();
        if required.is_universal() || required.is_empty() {
            return true;
        }
        required.intersects(query_features)
    }
}

/// Per-rule usage statistics for adaptive promotion/demotion.
///
/// Tracks lifetime and recent effectiveness, enabling the advisor
/// to promote high-performing rules to the "always try" tier and
/// demote chronic underperformers to the "try last" tier.
#[derive(Debug, Clone)]
pub struct RuleUsageStats {
    /// Total applications observed across all optimization runs.
    pub lifetime_hits: u64,
    /// Total misses observed across all optimization runs.
    pub lifetime_misses: u64,
    /// EWMA success rate (smoothed over time).
    pub ewma_success_rate: f64,
    /// Number of times this rule was promoted.
    pub promotion_count: u32,
    /// Number of times this rule was demoted.
    pub demotion_count: u32,
    /// Current tier: higher = tried earlier.
    pub tier: RuleTier,
}

impl RuleUsageStats {
    /// Create fresh stats for a newly observed rule.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lifetime_hits: 0,
            lifetime_misses: 0,
            ewma_success_rate: 0.5, // Neutral prior
            promotion_count: 0,
            demotion_count: 0,
            tier: RuleTier::Standard,
        }
    }

    /// Lifetime success rate.
    #[must_use]
    pub fn lifetime_success_rate(&self) -> f64 {
        let total = self.lifetime_hits + self.lifetime_misses;
        if total == 0 {
            return 0.5; // No data, neutral
        }
        self.lifetime_hits as f64 / total as f64
    }

    /// Update the EWMA with a new observation.
    pub fn update_ewma(&mut self, hit: bool, alpha: f64) {
        let observation = if hit { 1.0 } else { 0.0 };
        self.ewma_success_rate =
            alpha * observation + (1.0 - alpha) * self.ewma_success_rate;
        if hit {
            self.lifetime_hits += 1;
        } else {
            self.lifetime_misses += 1;
        }
    }
}

impl Default for RuleUsageStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Rule tier for adaptive scheduling.
///
/// Higher tiers are tried earlier in the optimization pipeline.
/// Rules move between tiers based on observed effectiveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuleTier {
    /// Demoted: tried last, may be skipped under tight budgets.
    Demoted = 0,
    /// Standard: default tier for all rules.
    Standard = 1,
    /// Promoted: tried first, historically effective.
    Promoted = 2,
}

/// Thresholds controlling tier transitions.
#[derive(Debug, Clone)]
pub struct TierThresholds {
    /// EWMA rate above which a rule is promoted.
    pub promotion_threshold: f64,
    /// EWMA rate below which a rule is demoted.
    pub demotion_threshold: f64,
    /// Minimum total observations before tier changes apply.
    pub min_observations: u64,
}

impl Default for TierThresholds {
    fn default() -> Self {
        Self {
            promotion_threshold: 0.3,
            demotion_threshold: 0.02,
            min_observations: 20,
        }
    }
}

/// Configuration for the rule advisor.
#[derive(Debug, Clone)]
pub struct RuleAdvisorConfig {
    /// Target database engine name (e.g. "postgresql", "mysql").
    /// Empty string means generic/all databases.
    pub database_name: String,
    /// Whether GPU hardware is available.
    pub has_gpu: bool,
    /// Whether this is a distributed (multi-node) deployment.
    pub is_distributed: bool,
    /// Enable Stage 3 learned ranking.
    pub enable_learning: bool,
    /// Path for the knowledge persistence file.
    /// Defaults to `~/.ra/rule-knowledge.json`.
    pub knowledge_path: Option<std::path::PathBuf>,
    /// Minimum observations before a rule can be deprioritized.
    pub min_observations: u32,
    /// EWMA smoothing factor (alpha) for effectiveness updates.
    pub ewma_alpha: f64,
    /// Effectiveness threshold below which rules are deprioritized.
    pub effectiveness_threshold: f64,
}

impl Default for RuleAdvisorConfig {
    fn default() -> Self {
        Self {
            database_name: String::new(),
            has_gpu: false,
            is_distributed: false,
            enable_learning: false,
            knowledge_path: None,
            min_observations: 10,
            ewma_alpha: 0.1,
            effectiveness_threshold: 0.01,
        }
    }
}

/// A rule slot surviving Stage 1 context elimination.
struct RuleSlot {
    label: &'static str,
    annotation: RuleAnnotation,
    rules: Vec<Rewrite<RelLang, RelAnalysis>>,
}

impl RuleApplicability for RuleSlot {
    fn required_features(&self) -> QueryFeatureSet {
        self.annotation.required_features
    }

    fn target_databases(&self) -> &[&str] {
        &self.annotation.databases
    }
}

/// Statistics about which rules were filtered at each stage.
#[derive(Debug, Clone, Default)]
pub struct AdvisorStats {
    /// Total rules available before any filtering.
    pub total_rules: usize,
    /// Rules remaining after Stage 1 (context elimination).
    pub after_stage1: usize,
    /// Rules remaining after Stage 2 (query-shape elimination).
    pub after_stage2: usize,
    /// Rules remaining after Stage 3 (learned ranking/exclusion).
    pub after_stage3: usize,
    /// Labels of groups eliminated in Stage 1.
    pub stage1_eliminated: Vec<String>,
    /// Labels of groups eliminated in Stage 2.
    pub stage2_eliminated: Vec<String>,
    /// Rules removed by budget-driven adaptive filtering.
    pub adaptive_removed: usize,
    /// Rules removed by consolidation exclusion.
    pub consolidation_removed: usize,
    /// Whether adaptive learning was active for this selection.
    pub adaptive_learning_active: bool,
}

/// Summary of rule distribution across tiers.
#[derive(Debug, Clone, Default)]
pub struct TierSummary {
    /// Rules in the promoted tier.
    pub promoted: usize,
    /// Rules in the standard tier.
    pub standard: usize,
    /// Rules in the demoted tier.
    pub demoted: usize,
}

/// Per-iteration adaptive tracking state.
///
/// Monitors rule success rates within a single optimization run and
/// dynamically adjusts rule availability between iterations, similar
/// to JIT compilation's tiered optimization.
#[derive(Debug, Clone)]
pub struct AdaptiveState {
    /// Per-rule hit/miss counters within the current optimization run.
    iteration_hits: HashMap<String, u32>,
    iteration_misses: HashMap<String, u32>,
    /// Rules demoted during this optimization run.
    demoted: Vec<String>,
    /// Number of iterations tracked so far.
    iterations_tracked: u32,
}

impl AdaptiveState {
    /// Create a fresh adaptive state for a new optimization run.
    #[must_use]
    pub fn new() -> Self {
        Self {
            iteration_hits: HashMap::new(),
            iteration_misses: HashMap::new(),
            demoted: Vec::new(),
            iterations_tracked: 0,
        }
    }

    /// Record a rule application (it matched and fired).
    pub fn record_hit(&mut self, rule_name: &str) {
        *self.iteration_hits.entry(rule_name.to_string()).or_insert(0) += 1;
    }

    /// Record a rule miss (it was available but didn't match).
    pub fn record_miss(&mut self, rule_name: &str) {
        *self.iteration_misses.entry(rule_name.to_string()).or_insert(0) += 1;
    }

    /// Advance to the next iteration.
    pub fn advance_iteration(&mut self) {
        self.iterations_tracked += 1;
    }

    /// Compute the current success rate for a rule.
    #[must_use]
    pub fn success_rate(&self, rule_name: &str) -> Option<f64> {
        let hits = self.iteration_hits.get(rule_name).copied().unwrap_or(0);
        let misses = self.iteration_misses.get(rule_name).copied().unwrap_or(0);
        let total = hits + misses;
        if total == 0 {
            return None;
        }
        Some(f64::from(hits) / f64::from(total))
    }

    /// Identify rules that should be demoted based on current run data.
    ///
    /// Rules with enough observations and success rate below the threshold
    /// are candidates for demotion within this optimization run.
    fn identify_demotions(
        &mut self,
        threshold: f64,
        min_observations: u32,
    ) -> Vec<String> {
        let mut new_demotions = Vec::new();

        for (name, &misses) in &self.iteration_misses {
            let hits = self.iteration_hits.get(name).copied().unwrap_or(0);
            let total = hits + misses;
            if total < min_observations {
                continue;
            }
            let rate = f64::from(hits) / f64::from(total);
            if rate < threshold && !self.demoted.contains(name) {
                new_demotions.push(name.clone());
            }
        }

        for name in &new_demotions {
            self.demoted.push(name.clone());
        }

        new_demotions
    }

    /// Get the list of rules demoted during this run.
    #[must_use]
    pub fn demoted_rules(&self) -> &[String] {
        &self.demoted
    }

    /// Number of iterations tracked.
    #[must_use]
    pub fn iterations_tracked(&self) -> u32 {
        self.iterations_tracked
    }

    /// Reset for a new optimization run.
    pub fn reset(&mut self) {
        self.iteration_hits.clear();
        self.iteration_misses.clear();
        self.demoted.clear();
        self.iterations_tracked = 0;
    }
}

impl Default for AdaptiveState {
    fn default() -> Self {
        Self::new()
    }
}

/// Detailed outcome of a single rule during optimization.
#[derive(Debug, Clone)]
pub struct RuleOutcome {
    /// Rule name.
    pub name: String,
    /// Whether the rule pattern matched at least once.
    pub matched: bool,
    /// Whether matching led to a cost improvement.
    pub improved: bool,
}

/// The rule advisor: three-stage filtering and ranking pipeline.
pub struct RuleAdvisor {
    config: RuleAdvisorConfig,
    /// Rule groups that survived Stage 1 (context elimination).
    context_slots: Vec<RuleSlot>,
    /// Total rule count before Stage 1.
    total_rule_count: usize,
    /// Learned knowledge store (Stage 3).
    knowledge: Option<RuleKnowledge>,
    /// Most recent advisor stats.
    last_stats: AdvisorStats,
    /// Per-run adaptive tracking.
    adaptive_state: AdaptiveState,
    /// Optional consolidator for long-term rule set optimization.
    consolidator: Option<RuleConsolidator>,
    /// Per-rule lifetime usage statistics for tier promotion/demotion.
    usage_stats: HashMap<String, RuleUsageStats>,
    /// Thresholds controlling tier transitions.
    tier_thresholds: TierThresholds,
}

impl RuleAdvisor {
    /// Create a new rule advisor by running Stage 1 context elimination.
    ///
    /// Stage 1 filters rule groups based on the target database engine
    /// and hardware capabilities. This is done once per optimizer
    /// instance.
    #[must_use]
    pub fn new(config: RuleAdvisorConfig) -> Self {
        let annotated = crate::rewrite::all_rules_annotated();
        let total_rule_count: usize = annotated.iter().map(|g| g.rules.len()).sum();

        let mut context_slots = Vec::with_capacity(annotated.len());
        let mut stage1_eliminated = Vec::new();

        for group in annotated {
            if Self::passes_context_filter(&group, &config) {
                context_slots.push(RuleSlot {
                    label: group.label,
                    annotation: group.annotation,
                    rules: group.rules,
                });
            } else {
                stage1_eliminated.push(group.label.to_string());
            }
        }

        let after_stage1: usize = context_slots.iter().map(|s| s.rules.len()).sum();

        info!(
            "Rule advisor Stage 1: {} -> {} rules ({} groups eliminated: [{}])",
            total_rule_count,
            after_stage1,
            stage1_eliminated.len(),
            stage1_eliminated.join(", "),
        );

        // Load knowledge store if learning is enabled
        let knowledge = if config.enable_learning {
            let path = config.knowledge_path.clone().unwrap_or_else(|| {
                let mut p = dirs_home().unwrap_or_default();
                p.push(".ra");
                p.push("rule-knowledge.json");
                p
            });
            match RuleKnowledge::load(&path) {
                Ok(k) => {
                    info!("Loaded rule knowledge: {} shape buckets", k.bucket_count(),);
                    Some(k)
                }
                Err(e) => {
                    debug!("No existing rule knowledge ({}), starting fresh", e);
                    Some(RuleKnowledge::new())
                }
            }
        } else {
            None
        };

        let last_stats = AdvisorStats {
            total_rules: total_rule_count,
            after_stage1,
            stage1_eliminated,
            ..AdvisorStats::default()
        };

        Self {
            config,
            context_slots,
            total_rule_count,
            knowledge,
            last_stats,
            adaptive_state: AdaptiveState::new(),
            consolidator: None,
            usage_stats: HashMap::new(),
            tier_thresholds: TierThresholds::default(),
        }
    }

    /// Attach a rule consolidator for long-term effectiveness tracking.
    pub fn with_consolidator(&mut self, consolidator: RuleConsolidator) {
        self.consolidator = Some(consolidator);
    }

    /// Get the adaptive state for external iteration tracking.
    #[must_use]
    pub fn adaptive_state(&self) -> &AdaptiveState {
        &self.adaptive_state
    }

    /// Get a mutable reference to the adaptive state.
    pub fn adaptive_state_mut(&mut self) -> &mut AdaptiveState {
        &mut self.adaptive_state
    }

    /// Get the rule consolidator, if attached.
    #[must_use]
    pub fn consolidator(&self) -> Option<&RuleConsolidator> {
        self.consolidator.as_ref()
    }

    /// Get a mutable reference to the consolidator, if attached.
    pub fn consolidator_mut(&mut self) -> Option<&mut RuleConsolidator> {
        self.consolidator.as_mut()
    }

    /// Set custom tier thresholds for rule promotion/demotion.
    pub fn with_tier_thresholds(&mut self, thresholds: TierThresholds) {
        self.tier_thresholds = thresholds;
    }

    /// Get the per-rule usage statistics.
    #[must_use]
    pub fn usage_stats(&self) -> &HashMap<String, RuleUsageStats> {
        &self.usage_stats
    }

    /// Get the usage stats for a specific rule.
    #[must_use]
    pub fn rule_usage(&self, name: &str) -> Option<&RuleUsageStats> {
        self.usage_stats.get(name)
    }

    /// Get the current tier thresholds.
    #[must_use]
    pub fn tier_thresholds(&self) -> &TierThresholds {
        &self.tier_thresholds
    }

    /// Update tier assignments based on accumulated usage stats.
    ///
    /// Promotes rules with high EWMA success rates and demotes those
    /// below the threshold, provided they have enough observations.
    /// Returns the number of tier changes made.
    pub fn update_tiers(&mut self) -> usize {
        let thresholds = &self.tier_thresholds;
        let mut changes = 0;

        for stats in self.usage_stats.values_mut() {
            let total = stats.lifetime_hits + stats.lifetime_misses;
            if total < thresholds.min_observations {
                continue;
            }

            let new_tier = if stats.ewma_success_rate
                >= thresholds.promotion_threshold
            {
                RuleTier::Promoted
            } else if stats.ewma_success_rate
                <= thresholds.demotion_threshold
            {
                RuleTier::Demoted
            } else {
                RuleTier::Standard
            };

            if new_tier != stats.tier {
                match new_tier {
                    RuleTier::Promoted => stats.promotion_count += 1,
                    RuleTier::Demoted => stats.demotion_count += 1,
                    RuleTier::Standard => {}
                }
                stats.tier = new_tier;
                changes += 1;
            }
        }

        if changes > 0 {
            debug!("Updated {} rule tier assignments", changes);
        }

        changes
    }

    /// Get a summary of rules by tier.
    #[must_use]
    pub fn tier_summary(&self) -> TierSummary {
        let mut promoted = 0usize;
        let mut standard = 0usize;
        let mut demoted = 0usize;

        for stats in self.usage_stats.values() {
            match stats.tier {
                RuleTier::Promoted => promoted += 1,
                RuleTier::Standard => standard += 1,
                RuleTier::Demoted => demoted += 1,
            }
        }

        TierSummary {
            promoted,
            standard,
            demoted,
        }
    }

    /// Select rules for a specific query expression (Stages 2 + 3).
    ///
    /// Runs query-shape elimination and learned ranking on the Stage 1
    /// survivors, returning a priority-sorted rule set ready for the
    /// e-graph runner.
    pub fn select_rules(&mut self, expr: &RelExpr) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        self.select_rules_inner(
            expr,
            self.config.enable_learning,
            self.config.min_observations,
            self.config.effectiveness_threshold,
            None,
        )
    }

    /// Select rules using a `ResourceBudget` for workload-specific behavior.
    ///
    /// The budget's [`RuleSelectionBehavior`] controls:
    /// - Whether adaptive learning is active for this query
    /// - The success rate threshold for rule deprioritization
    /// - The minimum observation count before filtering applies
    /// - Maximum rules per iteration (caps the returned set)
    ///
    /// This method also applies consolidation exclusions if a
    /// consolidator is attached, and uses the adaptive state to
    /// demote rules that are underperforming within the current
    /// optimization run.
    pub fn select_rules_with_budget(
        &mut self,
        expr: &RelExpr,
        budget: &ResourceBudget,
    ) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        let behavior = &budget.rule_selection;

        // Determine learning parameters from budget
        let use_learning = self.config.enable_learning || behavior.adaptive_learning;
        let threshold = if behavior.success_rate_threshold > 0.0 {
            behavior.success_rate_threshold
        } else {
            self.config.effectiveness_threshold
        };
        let min_obs = behavior.min_observations;

        let mut rules = self.select_rules_inner(
            expr,
            use_learning,
            min_obs,
            threshold,
            Some(behavior),
        );

        // Apply consolidation exclusions
        let consolidation_removed = if let Some(ref consolidator) = self.consolidator {
            let before = rules.len();
            rules = consolidator.filter_excluded(rules);
            before - rules.len()
        } else {
            0
        };

        // Apply adaptive demotion (rules underperforming this run)
        let adaptive_removed = if behavior.adaptive_learning {
            let demoted = self.adaptive_state.demoted_rules();
            if demoted.is_empty() {
                0
            } else {
                let before = rules.len();
                rules.retain(|r| {
                    !demoted.iter().any(|d| d == r.name.as_str())
                });
                before - rules.len()
            }
        } else {
            0
        };

        // Apply max rules per iteration cap
        if let Some(max) = behavior.max_rules_per_iteration {
            if rules.len() > max {
                rules.truncate(max);
            }
        }

        // Update extended stats
        self.last_stats.adaptive_removed = adaptive_removed;
        self.last_stats.consolidation_removed = consolidation_removed;
        self.last_stats.adaptive_learning_active = behavior.adaptive_learning;

        if adaptive_removed > 0 || consolidation_removed > 0 {
            debug!(
                "Budget-driven filtering: {} adaptive demotions, {} consolidation exclusions",
                adaptive_removed, consolidation_removed,
            );
        }

        rules
    }

    /// Core rule selection logic shared by both `select_rules` and
    /// `select_rules_with_budget`.
    fn select_rules_inner(
        &mut self,
        expr: &RelExpr,
        use_learning: bool,
        min_observations: u32,
        effectiveness_threshold: f64,
        behavior: Option<&RuleSelectionBehavior>,
    ) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        // Analyze query features
        let pattern = LazyQueryPattern::analyze(expr);
        let query_features = QueryFeatureSet::from_pattern(&pattern);

        // Determine whether fact-based filtering is active
        let use_fact_filtering = behavior
            .is_some_and(|b| b.fact_based_filtering);

        // Stage 2: query-shape elimination with RuleApplicability
        let mut selected = Vec::with_capacity(200);
        let mut stage2_eliminated = Vec::new();

        let default_behavior = RuleSelectionBehavior::default();
        let active_behavior = behavior.unwrap_or(&default_behavior);

        for slot in &self.context_slots {
            let passes = if use_fact_filtering {
                // Use the RuleApplicability trait for dynamic filtering
                slot.is_applicable(query_features, active_behavior)
            } else {
                Self::passes_shape_filter(&slot.annotation, query_features)
            };

            if passes {
                selected.extend(slot.rules.clone());
            } else {
                stage2_eliminated.push(slot.label.to_string());
            }
        }

        let after_stage2 = selected.len();

        if !stage2_eliminated.is_empty() {
            debug!(
                "Rule advisor Stage 2: {} -> {} rules (eliminated: [{}])",
                self.last_stats.after_stage1,
                after_stage2,
                stage2_eliminated.join(", "),
            );
        }

        // Stage 3: learned ranking
        let after_stage3 = if use_learning {
            if let Some(ref knowledge) = self.knowledge {
                let shape_key = ShapeKeyBucket::from_expr(expr);
                selected = knowledge.rank_and_filter(
                    selected,
                    &shape_key,
                    min_observations,
                    effectiveness_threshold,
                );
                selected.len()
            } else {
                // Learning requested but no knowledge store: sort by priority
                selected = crate::rule_priority::sort_rules_by_priority(selected);
                selected.len()
            }
        } else {
            // No learning: just apply priority sorting
            selected = crate::rule_priority::sort_rules_by_priority(selected);
            selected.len()
        };

        // Stage 3.5: tier-aware reordering
        //
        // When usage stats are available, boost promoted rules to
        // the front and push demoted rules to the back, preserving
        // relative order within each tier.
        if !self.usage_stats.is_empty() {
            let stats = &self.usage_stats;
            selected.sort_by(|a, b| {
                let tier_a = stats
                    .get(a.name.as_str())
                    .map_or(RuleTier::Standard, |s| s.tier);
                let tier_b = stats
                    .get(b.name.as_str())
                    .map_or(RuleTier::Standard, |s| s.tier);
                // Higher tier first (Promoted > Standard > Demoted)
                tier_b.cmp(&tier_a)
            });
        }

        // Update stats
        self.last_stats.after_stage2 = after_stage2;
        self.last_stats.after_stage3 = after_stage3;
        self.last_stats.stage2_eliminated = stage2_eliminated;
        self.last_stats.adaptive_removed = 0;
        self.last_stats.consolidation_removed = 0;
        self.last_stats.adaptive_learning_active = use_learning;

        info!(
            "Rule advisor: {} -> {} -> {} -> {} rules",
            self.total_rule_count, self.last_stats.after_stage1, after_stage2, after_stage3,
        );

        selected
    }

    /// Record optimization outcome for Stage 3 learning.
    ///
    /// Called after optimization completes with the set of rules that
    /// were applied and the resulting cost improvement.
    pub fn record_outcome(
        &mut self,
        expr: &RelExpr,
        applied_rule_names: &[String],
        _best_cost: f64,
    ) {
        if let Some(ref mut knowledge) = self.knowledge {
            let shape_key = ShapeKeyBucket::from_expr(expr);

            // Record which rules were applied for this shape
            for name in applied_rule_names {
                knowledge.record_application(&shape_key, name);
            }

            // Persist knowledge
            let path = self.config.knowledge_path.clone().unwrap_or_else(|| {
                let mut p = dirs_home().unwrap_or_default();
                p.push(".ra");
                p.push("rule-knowledge.json");
                p
            });
            if let Err(e) = knowledge.save(&path) {
                debug!("Failed to persist rule knowledge: {}", e);
            }
        }
    }

    /// Record detailed per-rule outcomes for both learning and
    /// adaptive state.
    ///
    /// This provides richer feedback than `record_outcome` by tracking
    /// individual rule match/improvement results, feeding the
    /// persistent knowledge store, the per-run adaptive state, and
    /// the lifetime usage statistics for tier promotion/demotion.
    pub fn record_detailed_outcome(
        &mut self,
        expr: &RelExpr,
        outcomes: &[RuleOutcome],
    ) {
        let shape_key = ShapeKeyBucket::from_expr(expr);

        // Update persistent knowledge
        if let Some(ref mut knowledge) = self.knowledge {
            for outcome in outcomes {
                if outcome.matched {
                    knowledge.record_application(&shape_key, &outcome.name);
                } else {
                    knowledge.record_miss(&shape_key, &outcome.name);
                }
            }

            let path = self.config.knowledge_path.clone().unwrap_or_else(|| {
                let mut p = dirs_home().unwrap_or_default();
                p.push(".ra");
                p.push("rule-knowledge.json");
                p
            });
            if let Err(e) = knowledge.save(&path) {
                debug!("Failed to persist rule knowledge: {}", e);
            }
        }

        // Update adaptive state and usage stats
        let alpha = self.config.ewma_alpha;
        for outcome in outcomes {
            if outcome.matched {
                self.adaptive_state.record_hit(&outcome.name);
            } else {
                self.adaptive_state.record_miss(&outcome.name);
            }

            // Update lifetime usage stats
            self.usage_stats
                .entry(outcome.name.clone())
                .or_default()
                .update_ewma(outcome.matched, alpha);
        }

        // Update consolidator if attached
        if let Some(ref mut consolidator) = self.consolidator {
            for outcome in outcomes {
                consolidator.record_application(
                    &outcome.name,
                    &shape_key,
                    outcome.matched,
                    outcome.improved,
                );
            }
        }
    }

    /// Run adaptive demotion: identify rules to demote based on
    /// current-run observations.
    ///
    /// Returns the names of newly demoted rules (rules that were not
    /// previously demoted but now fall below the threshold).
    pub fn run_adaptive_demotion(
        &mut self,
        behavior: &RuleSelectionBehavior,
    ) -> Vec<String> {
        if !behavior.adaptive_learning {
            return Vec::new();
        }

        let threshold = if behavior.success_rate_threshold > 0.0 {
            behavior.success_rate_threshold
        } else {
            self.config.effectiveness_threshold
        };

        self.adaptive_state.identify_demotions(
            threshold,
            behavior.min_observations,
        )
    }

    /// Reset the adaptive state for a new optimization run.
    pub fn reset_adaptive_state(&mut self) {
        self.adaptive_state.reset();
    }

    /// Get the most recent advisor statistics.
    #[must_use]
    pub fn stats(&self) -> &AdvisorStats {
        &self.last_stats
    }

    /// Get the knowledge store, if learning is enabled.
    #[must_use]
    pub fn knowledge(&self) -> Option<&RuleKnowledge> {
        self.knowledge.as_ref()
    }

    /// Stage 1: check if a rule group passes context filtering.
    fn passes_context_filter(group: &AnnotatedRuleGroup, config: &RuleAdvisorConfig) -> bool {
        let ann = &group.annotation;

        // Database scope check: if the group declares specific databases,
        // the target must be one of them (or target is empty = generic).
        if !ann.databases.is_empty() && !config.database_name.is_empty() {
            let target_lower = config.database_name.to_lowercase();
            if !ann
                .databases
                .iter()
                .any(|db| db.eq_ignore_ascii_case(&target_lower))
            {
                return false;
            }
        }

        true
    }

    /// Stage 2: check if a rule group's required features overlap
    /// with the query's detected features.
    fn passes_shape_filter(annotation: &RuleAnnotation, query_features: QueryFeatureSet) -> bool {
        let required = annotation.required_features;

        // Universal rules always pass
        if required.is_universal() {
            return true;
        }

        // If the rule requires no features, it passes
        if required.is_empty() {
            return true;
        }

        // Pass if any required feature is present in the query
        required.intersects(query_features)
    }
}

impl std::fmt::Debug for RuleAdvisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleAdvisor")
            .field("config", &self.config)
            .field("context_slots", &self.context_slots.len())
            .field("total_rule_count", &self.total_rule_count)
            .field("has_knowledge", &self.knowledge.is_some())
            .field("has_consolidator", &self.consolidator.is_some())
            .field("tracked_rules", &self.usage_stats.len())
            .field("tier_summary", &self.tier_summary())
            .finish_non_exhaustive()
    }
}

/// Get the user's home directory.
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_passes_all_rules() {
        let config = RuleAdvisorConfig::default();
        let advisor = RuleAdvisor::new(config);
        // With no database filter, all groups should survive Stage 1
        assert_eq!(advisor.last_stats.after_stage1, advisor.total_rule_count,);
    }

    #[test]
    fn postgresql_config_excludes_documentdb_and_oracle() {
        let config = RuleAdvisorConfig {
            database_name: "postgresql".to_string(),
            ..RuleAdvisorConfig::default()
        };
        let advisor = RuleAdvisor::new(config);
        let eliminated = &advisor.last_stats.stage1_eliminated;

        assert!(
            eliminated.iter().any(|l| l == "documentdb-bson"),
            "documentdb rules should be eliminated for postgresql"
        );
        assert!(
            eliminated.iter().any(|l| l == "oracle-json-duality"),
            "oracle duality rules should be eliminated for postgresql"
        );
    }

    #[test]
    fn plain_scan_excludes_specialty_rules() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        // A simple scan query has no JSON, BSON, vector, FTS, XML features
        let expr = RelExpr::scan("users");
        let rules = advisor.select_rules(&expr);

        let eliminated = &advisor.last_stats.stage2_eliminated;
        // Vector, FTS, hybrid, XML, cast groups should be excluded
        assert!(
            eliminated.iter().any(|l| l == "vector-search"),
            "vector rules should be eliminated for plain scan"
        );
        assert!(
            eliminated.iter().any(|l| l == "full-text-search"),
            "FTS rules should be eliminated for plain scan"
        );

        // Fewer rules than total
        assert!(
            rules.len() < advisor.total_rule_count,
            "expected fewer rules ({}) than total ({})",
            rules.len(),
            advisor.total_rule_count,
        );
    }

    #[test]
    fn join_query_includes_join_rules() {
        use ra_core::algebra::JoinType;
        use ra_core::expr::{BinOp, ColumnRef, Expr};

        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("a"))),
                right: Box::new(Expr::Column(ColumnRef::new("b"))),
            },
            left: Box::new(RelExpr::scan("t1")),
            right: Box::new(RelExpr::scan("t2")),
        };
        let rules = advisor.select_rules(&expr);

        // Join rules should be included
        let eliminated = &advisor.last_stats.stage2_eliminated;
        assert!(
            !eliminated.iter().any(|l| l == "join-reordering"),
            "join-reordering should NOT be eliminated for a join query"
        );
        assert!(!rules.is_empty());
    }

    #[test]
    fn shape_filter_universal_always_passes() {
        let ann = RuleAnnotation {
            required_features: QueryFeatureSet::UNIVERSAL,
            databases: vec![],
        };
        assert!(RuleAdvisor::passes_shape_filter(
            &ann,
            QueryFeatureSet::EMPTY
        ));
    }

    #[test]
    fn shape_filter_no_overlap_fails() {
        let ann = RuleAnnotation {
            required_features: QueryFeatureSet::HAS_VECTOR_DISTANCE,
            databases: vec![],
        };
        assert!(!RuleAdvisor::passes_shape_filter(
            &ann,
            QueryFeatureSet::HAS_JOIN
        ));
    }

    #[test]
    fn shape_filter_overlap_passes() {
        let ann = RuleAnnotation {
            required_features: QueryFeatureSet::HAS_JOIN.union(QueryFeatureSet::HAS_AGGREGATE),
            databases: vec![],
        };
        assert!(RuleAdvisor::passes_shape_filter(
            &ann,
            QueryFeatureSet::HAS_JOIN
        ));
    }

    // ---- Adaptive state tests ----

    #[test]
    fn adaptive_state_records_hits_and_misses() {
        let mut state = AdaptiveState::new();
        state.record_hit("rule-a");
        state.record_hit("rule-a");
        state.record_miss("rule-a");
        state.record_miss("rule-b");

        let rate_a = state.success_rate("rule-a");
        assert!(rate_a.is_some());
        // 2 hits / 3 total
        assert!((rate_a.unwrap() - 2.0 / 3.0).abs() < 1e-6);

        let rate_b = state.success_rate("rule-b");
        assert!(rate_b.is_some());
        assert!((rate_b.unwrap()).abs() < f64::EPSILON);
    }

    #[test]
    fn adaptive_state_no_data_returns_none() {
        let state = AdaptiveState::new();
        assert!(state.success_rate("unknown-rule").is_none());
    }

    #[test]
    fn adaptive_state_demotion() {
        let mut state = AdaptiveState::new();

        // rule-a: 0% success rate with 20 observations
        for _ in 0..20 {
            state.record_miss("rule-a");
        }
        // rule-b: 100% success rate
        for _ in 0..20 {
            state.record_hit("rule-b");
        }

        let demotions = state.identify_demotions(0.05, 10);
        assert!(demotions.contains(&"rule-a".to_string()));
        assert!(!demotions.contains(&"rule-b".to_string()));
        assert_eq!(state.demoted_rules().len(), 1);
    }

    #[test]
    fn adaptive_state_demotion_respects_min_observations() {
        let mut state = AdaptiveState::new();

        // Only 3 observations (below min of 10)
        for _ in 0..3 {
            state.record_miss("rule-a");
        }

        let demotions = state.identify_demotions(0.05, 10);
        assert!(demotions.is_empty());
    }

    #[test]
    fn adaptive_state_no_duplicate_demotions() {
        let mut state = AdaptiveState::new();

        for _ in 0..20 {
            state.record_miss("rule-a");
        }

        let first = state.identify_demotions(0.05, 10);
        assert_eq!(first.len(), 1);

        // Second call should not produce duplicates
        let second = state.identify_demotions(0.05, 10);
        assert!(second.is_empty());
        assert_eq!(state.demoted_rules().len(), 1);
    }

    #[test]
    fn adaptive_state_reset() {
        let mut state = AdaptiveState::new();
        state.record_hit("rule-a");
        state.advance_iteration();
        assert_eq!(state.iterations_tracked(), 1);

        state.reset();
        assert_eq!(state.iterations_tracked(), 0);
        assert!(state.success_rate("rule-a").is_none());
        assert!(state.demoted_rules().is_empty());
    }

    // ---- Budget-driven selection tests ----

    #[test]
    fn select_with_budget_caps_rules() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let expr = RelExpr::scan("users");

        // First, get the uncapped count
        let uncapped = advisor.select_rules(&expr);
        let uncapped_count = uncapped.len();
        assert!(uncapped_count > 5, "need enough rules to test capping");

        // Now select with a budget that caps at 5
        let budget = ResourceBudget::unlimited()
            .with_rule_selection(RuleSelectionBehavior {
                max_rules_per_iteration: Some(5),
                ..RuleSelectionBehavior::default()
            });
        let capped = advisor.select_rules_with_budget(&expr, &budget);
        assert_eq!(capped.len(), 5);
    }

    #[test]
    fn select_with_budget_no_cap_returns_all() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let expr = RelExpr::scan("users");
        let budget = ResourceBudget::unlimited();

        let uncapped = advisor.select_rules(&expr);
        let with_budget = advisor.select_rules_with_budget(&expr, &budget);

        assert_eq!(uncapped.len(), with_budget.len());
    }

    #[test]
    fn select_with_budget_reports_adaptive_stats() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let expr = RelExpr::scan("users");
        let budget = ResourceBudget::unlimited()
            .with_rule_selection(RuleSelectionBehavior {
                adaptive_learning: true,
                ..RuleSelectionBehavior::default()
            });

        let _ = advisor.select_rules_with_budget(&expr, &budget);
        assert!(advisor.stats().adaptive_learning_active);
    }

    #[test]
    fn record_detailed_outcome_updates_adaptive_state() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let expr = RelExpr::scan("users");
        let outcomes = vec![
            RuleOutcome {
                name: "filter-true".to_string(),
                matched: true,
                improved: true,
            },
            RuleOutcome {
                name: "null-eq".to_string(),
                matched: false,
                improved: false,
            },
        ];

        advisor.record_detailed_outcome(&expr, &outcomes);

        let rate = advisor.adaptive_state().success_rate("filter-true");
        assert!(rate.is_some());
        assert!((rate.unwrap() - 1.0).abs() < f64::EPSILON);

        let rate = advisor.adaptive_state().success_rate("null-eq");
        assert!(rate.is_some());
        assert!(rate.unwrap().abs() < f64::EPSILON);
    }

    #[test]
    fn run_adaptive_demotion_with_learning_disabled() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let behavior = RuleSelectionBehavior {
            adaptive_learning: false,
            ..RuleSelectionBehavior::default()
        };

        let demotions = advisor.run_adaptive_demotion(&behavior);
        assert!(demotions.is_empty());
    }

    #[test]
    fn advisor_with_consolidator() {
        use crate::rule_consolidation::{ConsolidationConfig, RuleConsolidator};

        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        assert!(advisor.consolidator().is_none());

        let consolidator = RuleConsolidator::new(ConsolidationConfig::default());
        advisor.with_consolidator(consolidator);

        assert!(advisor.consolidator().is_some());
    }

    // ---- RuleApplicability trait tests ----

    #[test]
    fn rule_applicability_universal_slot() {
        let slot = RuleSlot {
            label: "test-universal",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::UNIVERSAL,
                databases: vec![],
            },
            rules: vec![],
        };

        assert!(slot.is_applicable(
            QueryFeatureSet::EMPTY,
            &RuleSelectionBehavior::default(),
        ));
        assert!(slot.is_applicable(
            QueryFeatureSet::HAS_JOIN,
            &RuleSelectionBehavior::default(),
        ));
    }

    #[test]
    fn rule_applicability_feature_match() {
        let slot = RuleSlot {
            label: "test-join",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::HAS_JOIN,
                databases: vec![],
            },
            rules: vec![],
        };

        assert!(slot.is_applicable(
            QueryFeatureSet::HAS_JOIN,
            &RuleSelectionBehavior::default(),
        ));
        assert!(!slot.is_applicable(
            QueryFeatureSet::HAS_AGGREGATE,
            &RuleSelectionBehavior::default(),
        ));
    }

    #[test]
    fn rule_applicability_target_databases() {
        let slot = RuleSlot {
            label: "test-pg",
            annotation: RuleAnnotation {
                required_features: QueryFeatureSet::EMPTY,
                databases: vec!["postgresql"],
            },
            rules: vec![],
        };

        assert_eq!(slot.target_databases(), &["postgresql"]);
    }

    // ---- RuleUsageStats tests ----

    #[test]
    fn usage_stats_default_neutral() {
        let stats = RuleUsageStats::new();
        assert!((stats.lifetime_success_rate() - 0.5).abs() < f64::EPSILON);
        assert_eq!(stats.tier, RuleTier::Standard);
        assert_eq!(stats.promotion_count, 0);
        assert_eq!(stats.demotion_count, 0);
    }

    #[test]
    fn usage_stats_update_ewma_hit() {
        let mut stats = RuleUsageStats::new();
        stats.update_ewma(true, 0.5);

        // EWMA: 0.5 * 1.0 + 0.5 * 0.5 = 0.75
        assert!((stats.ewma_success_rate - 0.75).abs() < 1e-6);
        assert_eq!(stats.lifetime_hits, 1);
        assert_eq!(stats.lifetime_misses, 0);
    }

    #[test]
    fn usage_stats_update_ewma_miss() {
        let mut stats = RuleUsageStats::new();
        stats.update_ewma(false, 0.5);

        // EWMA: 0.5 * 0.0 + 0.5 * 0.5 = 0.25
        assert!((stats.ewma_success_rate - 0.25).abs() < 1e-6);
        assert_eq!(stats.lifetime_hits, 0);
        assert_eq!(stats.lifetime_misses, 1);
    }

    #[test]
    fn usage_stats_lifetime_rate() {
        let mut stats = RuleUsageStats::new();
        for _ in 0..3 {
            stats.update_ewma(true, 0.1);
        }
        for _ in 0..7 {
            stats.update_ewma(false, 0.1);
        }

        // 3 hits / 10 total = 0.3
        assert!((stats.lifetime_success_rate() - 0.3).abs() < f64::EPSILON);
    }

    // ---- Tier management tests ----

    #[test]
    fn tier_ordering() {
        assert!(RuleTier::Promoted > RuleTier::Standard);
        assert!(RuleTier::Standard > RuleTier::Demoted);
    }

    #[test]
    fn update_tiers_promotes_effective_rules() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        // Insert a rule with high EWMA
        advisor.usage_stats.insert(
            "high-rule".to_string(),
            RuleUsageStats {
                lifetime_hits: 30,
                lifetime_misses: 5,
                ewma_success_rate: 0.8,
                promotion_count: 0,
                demotion_count: 0,
                tier: RuleTier::Standard,
            },
        );

        let changes = advisor.update_tiers();
        assert_eq!(changes, 1);
        assert_eq!(
            advisor.usage_stats["high-rule"].tier,
            RuleTier::Promoted,
        );
        assert_eq!(advisor.usage_stats["high-rule"].promotion_count, 1);
    }

    #[test]
    fn update_tiers_demotes_ineffective_rules() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        // Insert a rule with very low EWMA
        advisor.usage_stats.insert(
            "low-rule".to_string(),
            RuleUsageStats {
                lifetime_hits: 0,
                lifetime_misses: 30,
                ewma_success_rate: 0.005,
                promotion_count: 0,
                demotion_count: 0,
                tier: RuleTier::Standard,
            },
        );

        let changes = advisor.update_tiers();
        assert_eq!(changes, 1);
        assert_eq!(
            advisor.usage_stats["low-rule"].tier,
            RuleTier::Demoted,
        );
        assert_eq!(advisor.usage_stats["low-rule"].demotion_count, 1);
    }

    #[test]
    fn update_tiers_ignores_insufficient_observations() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        // Only 5 observations (below min of 20)
        advisor.usage_stats.insert(
            "new-rule".to_string(),
            RuleUsageStats {
                lifetime_hits: 0,
                lifetime_misses: 5,
                ewma_success_rate: 0.001,
                promotion_count: 0,
                demotion_count: 0,
                tier: RuleTier::Standard,
            },
        );

        let changes = advisor.update_tiers();
        assert_eq!(changes, 0);
        assert_eq!(
            advisor.usage_stats["new-rule"].tier,
            RuleTier::Standard,
        );
    }

    #[test]
    fn update_tiers_no_change_for_standard_range() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        // EWMA in the "standard" range (between demotion and promotion)
        advisor.usage_stats.insert(
            "mid-rule".to_string(),
            RuleUsageStats {
                lifetime_hits: 10,
                lifetime_misses: 15,
                ewma_success_rate: 0.15,
                promotion_count: 0,
                demotion_count: 0,
                tier: RuleTier::Standard,
            },
        );

        let changes = advisor.update_tiers();
        assert_eq!(changes, 0);
    }

    #[test]
    fn tier_summary_counts_correctly() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        advisor.usage_stats.insert(
            "promoted-1".to_string(),
            RuleUsageStats {
                tier: RuleTier::Promoted,
                ..RuleUsageStats::new()
            },
        );
        advisor.usage_stats.insert(
            "promoted-2".to_string(),
            RuleUsageStats {
                tier: RuleTier::Promoted,
                ..RuleUsageStats::new()
            },
        );
        advisor.usage_stats.insert(
            "standard-1".to_string(),
            RuleUsageStats::new(),
        );
        advisor.usage_stats.insert(
            "demoted-1".to_string(),
            RuleUsageStats {
                tier: RuleTier::Demoted,
                ..RuleUsageStats::new()
            },
        );

        let summary = advisor.tier_summary();
        assert_eq!(summary.promoted, 2);
        assert_eq!(summary.standard, 1);
        assert_eq!(summary.demoted, 1);
    }

    // ---- Detailed outcome updates usage stats ----

    #[test]
    fn record_detailed_outcome_updates_usage_stats() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let expr = RelExpr::scan("users");
        let outcomes = vec![
            RuleOutcome {
                name: "rule-x".to_string(),
                matched: true,
                improved: true,
            },
            RuleOutcome {
                name: "rule-y".to_string(),
                matched: false,
                improved: false,
            },
        ];

        advisor.record_detailed_outcome(&expr, &outcomes);

        let stats_x = advisor.rule_usage("rule-x");
        assert!(stats_x.is_some());
        assert_eq!(stats_x.unwrap().lifetime_hits, 1);
        assert_eq!(stats_x.unwrap().lifetime_misses, 0);

        let stats_y = advisor.rule_usage("rule-y");
        assert!(stats_y.is_some());
        assert_eq!(stats_y.unwrap().lifetime_hits, 0);
        assert_eq!(stats_y.unwrap().lifetime_misses, 1);
    }

    #[test]
    fn custom_tier_thresholds() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        // Use very aggressive thresholds
        advisor.with_tier_thresholds(TierThresholds {
            promotion_threshold: 0.9,
            demotion_threshold: 0.1,
            min_observations: 5,
        });

        assert!(
            (advisor.tier_thresholds().promotion_threshold - 0.9).abs()
                < f64::EPSILON,
        );
    }

    // ---- Fact-based filtering via budget ----

    #[test]
    fn select_with_budget_fact_based_filtering() {
        let config = RuleAdvisorConfig::default();
        let mut advisor = RuleAdvisor::new(config);

        let expr = RelExpr::scan("users");

        // With fact_based_filtering enabled (uses RuleApplicability)
        let budget = ResourceBudget::unlimited()
            .with_rule_selection(RuleSelectionBehavior {
                fact_based_filtering: true,
                ..RuleSelectionBehavior::default()
            });

        let rules_with_facts = advisor.select_rules_with_budget(&expr, &budget);

        // With fact_based_filtering disabled
        let budget_no_facts = ResourceBudget::unlimited()
            .with_rule_selection(RuleSelectionBehavior {
                fact_based_filtering: false,
                ..RuleSelectionBehavior::default()
            });

        let rules_no_facts = advisor.select_rules_with_budget(
            &expr,
            &budget_no_facts,
        );

        // Both should produce valid (non-empty) results.
        // The exact count may differ since fact_based_filtering
        // uses the RuleApplicability trait which has the same
        // default behavior as the static filter for RuleSlot.
        assert!(!rules_with_facts.is_empty());
        assert!(!rules_no_facts.is_empty());
    }

    // ---- Default tier thresholds ----

    #[test]
    fn default_tier_thresholds() {
        let thresholds = TierThresholds::default();
        assert!(
            (thresholds.promotion_threshold - 0.3).abs() < f64::EPSILON,
        );
        assert!(
            (thresholds.demotion_threshold - 0.02).abs() < f64::EPSILON,
        );
        assert_eq!(thresholds.min_observations, 20);
    }
}

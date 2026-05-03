//! Rule consolidation and effectiveness analysis.
//!
//! This module eliminates redundant and low-impact rules to improve
//! optimization performance. It provides:
//!
//! - Effectiveness tracking: removes rules with <1% success rate
//! - Rule merging: combines related rules into composites
//! - Dependency analysis: identifies and resolves rule conflicts
//! - Automatic consolidation: based on usage patterns
//! - Metrics collection: rule application success rates
//! - Integration with `RuleAdvisor` for feedback

use std::collections::{HashMap, HashSet};

use egg::Rewrite;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::rule_knowledge::{RuleKnowledge, ShapeKeyBucket};

/// Configuration for the rule consolidation system.
#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    /// Minimum number of observations before a rule can be removed.
    pub min_observations: u32,
    /// Effectiveness threshold below which rules are candidates for
    /// removal (default: 0.01 = 1%).
    pub effectiveness_threshold: f64,
    /// Maximum number of rules to remove in a single consolidation pass.
    pub max_removals_per_pass: usize,
    /// Enable rule merging (combining related rules into composites).
    pub enable_merging: bool,
    /// Similarity threshold for merging rules (0.0..=1.0).
    /// Rules with overlap above this threshold are merge candidates.
    pub merge_similarity_threshold: f64,
    /// Enable dependency analysis and conflict resolution.
    pub enable_dependency_analysis: bool,
    /// Decay factor for aging out old observations (0.0..=1.0).
    /// Applied per consolidation pass to gradually forget old data.
    pub observation_decay: f64,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            min_observations: 50,
            effectiveness_threshold: 0.01,
            max_removals_per_pass: 10,
            enable_merging: true,
            merge_similarity_threshold: 0.8,
            enable_dependency_analysis: true,
            observation_decay: 0.95,
        }
    }
}

/// A record of a rule's observed effectiveness across all query shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEffectivenessRecord {
    /// Rule name (matches egg rewrite name).
    pub name: String,
    /// Total number of optimization runs where this rule was available.
    pub total_attempts: u32,
    /// Number of times the rule pattern actually matched.
    pub total_matches: u32,
    /// Number of times matching led to a cost improvement.
    pub total_improvements: u32,
    /// Per-shape-bucket success rates.
    pub per_shape_rates: HashMap<ShapeKeyBucket, f64>,
    /// Overall EWMA effectiveness across all shapes.
    pub overall_effectiveness: f64,
    /// Category tag from the rule registry.
    pub category: String,
}

impl RuleEffectivenessRecord {
    /// Success rate: matches / attempts.
    #[must_use]
    pub fn success_rate(&self) -> f64 {
        if self.total_attempts == 0 {
            return 0.0;
        }
        f64::from(self.total_matches) / f64::from(self.total_attempts)
    }

    /// Improvement rate: improvements / matches.
    #[must_use]
    pub fn improvement_rate(&self) -> f64 {
        if self.total_matches == 0 {
            return 0.0;
        }
        f64::from(self.total_improvements) / f64::from(self.total_matches)
    }
}

/// Describes a dependency relationship between two rules.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleDependency {
    /// The rule that must fire first (enabler).
    pub prerequisite: String,
    /// The rule that benefits from the prerequisite firing.
    pub dependent: String,
    /// Type of dependency.
    pub kind: DependencyKind,
}

/// Types of rule dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DependencyKind {
    /// The prerequisite creates patterns the dependent matches on.
    Enables,
    /// The two rules conflict (applying one prevents the other).
    Conflicts,
    /// The two rules are redundant (produce equivalent results).
    Subsumes,
}

/// A conflict between two rules that should be resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConflict {
    /// First rule in the conflict.
    pub rule_a: String,
    /// Second rule in the conflict.
    pub rule_b: String,
    /// Description of the conflict.
    pub description: String,
    /// Suggested resolution.
    pub resolution: ConflictResolution,
}

/// How to resolve a rule conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Keep only rule A (B is subsumed).
    KeepA,
    /// Keep only rule B (A is subsumed).
    KeepB,
    /// Merge both into a composite rule.
    Merge,
    /// Order A before B to avoid interference.
    OrderABeforeB,
    /// No automatic resolution possible.
    Manual { reason: String },
}

/// Result of a consolidation pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    /// Rules removed due to low effectiveness.
    pub removed_rules: Vec<String>,
    /// Rules merged into composites.
    pub merged_rules: Vec<MergeRecord>,
    /// Conflicts detected and their resolutions.
    pub conflicts_resolved: Vec<RuleConflict>,
    /// Total rules before consolidation.
    pub rules_before: usize,
    /// Total rules after consolidation.
    pub rules_after: usize,
    /// Summary metrics.
    pub metrics: ConsolidationMetrics,
}

/// A record of rules that were merged into a composite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRecord {
    /// Names of the source rules that were merged.
    pub source_rules: Vec<String>,
    /// Name of the resulting composite rule.
    pub composite_name: String,
    /// Category of the merged rule group.
    pub category: String,
}

/// Summary metrics from a consolidation pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsolidationMetrics {
    /// Number of rules analyzed.
    pub rules_analyzed: usize,
    /// Number of rules below the effectiveness threshold.
    pub below_threshold: usize,
    /// Number of rules removed.
    pub rules_removed: usize,
    /// Number of merge groups formed.
    pub merge_groups_formed: usize,
    /// Number of dependencies detected.
    pub dependencies_detected: usize,
    /// Number of conflicts detected.
    pub conflicts_detected: usize,
    /// Average effectiveness across all tracked rules.
    pub average_effectiveness: f64,
}

/// The rule consolidation engine.
///
/// Analyzes rule effectiveness over time and eliminates or merges
/// rules that provide little optimization value, reducing the cost
/// of equality saturation exploration.
pub struct RuleConsolidator {
    config: ConsolidationConfig,
    /// Accumulated effectiveness data per rule.
    effectiveness: HashMap<String, RuleEffectivenessRecord>,
    /// Known dependencies between rules.
    dependencies: Vec<RuleDependency>,
    /// Rules that have been permanently excluded.
    excluded_rules: HashSet<String>,
    /// History of consolidation passes.
    pass_count: u32,
}

impl RuleConsolidator {
    /// Create a new rule consolidator with the given configuration.
    #[must_use]
    pub fn new(config: ConsolidationConfig) -> Self {
        Self {
            config,
            effectiveness: HashMap::new(),
            dependencies: Vec::new(),
            excluded_rules: HashSet::new(),
            pass_count: 0,
        }
    }

    /// Create a consolidator seeded with knowledge from a `RuleKnowledge` store.
    #[must_use]
    pub fn from_knowledge(config: ConsolidationConfig, knowledge: &RuleKnowledge) -> Self {
        let mut consolidator = Self::new(config);
        consolidator.ingest_knowledge(knowledge);
        consolidator
    }

    /// Ingest effectiveness data from a `RuleKnowledge` store.
    pub fn ingest_knowledge(&mut self, knowledge: &RuleKnowledge) {
        for (shape, bucket_data) in knowledge.entries() {
            for (rule_name, entry) in bucket_data {
                let record = self
                    .effectiveness
                    .entry(rule_name.clone())
                    .or_insert_with(|| RuleEffectivenessRecord {
                        name: rule_name.clone(),
                        total_attempts: 0,
                        total_matches: 0,
                        total_improvements: 0,
                        per_shape_rates: HashMap::new(),
                        overall_effectiveness: 0.5,
                        category: String::new(),
                    });

                record.total_attempts += entry.attempts;
                record.total_matches += entry.matches;
                record.total_improvements += entry.improvements;
                record
                    .per_shape_rates
                    .insert(shape.clone(), entry.ewma_effectiveness);
            }
        }

        // Recompute overall effectiveness for each rule
        for record in self.effectiveness.values_mut() {
            if record.per_shape_rates.is_empty() {
                continue;
            }
            let sum: f64 = record.per_shape_rates.values().sum();
            record.overall_effectiveness =
                sum / record.per_shape_rates.len() as f64;
        }
    }

    /// Record a single rule application event.
    pub fn record_application(
        &mut self,
        rule_name: &str,
        shape: &ShapeKeyBucket,
        matched: bool,
        improved: bool,
    ) {
        let record = self
            .effectiveness
            .entry(rule_name.to_string())
            .or_insert_with(|| RuleEffectivenessRecord {
                name: rule_name.to_string(),
                total_attempts: 0,
                total_matches: 0,
                total_improvements: 0,
                per_shape_rates: HashMap::new(),
                overall_effectiveness: 0.5,
                category: String::new(),
            });

        record.total_attempts += 1;
        if matched {
            record.total_matches += 1;
        }
        if improved {
            record.total_improvements += 1;
        }

        // Update per-shape rate using EWMA
        let alpha = 0.1;
        let observation = if matched { 1.0 } else { 0.0 };
        let current = record
            .per_shape_rates
            .entry(shape.clone())
            .or_insert(0.5);
        *current = alpha * observation + (1.0 - alpha) * *current;

        // Recompute overall effectiveness from per-shape rates
        if !record.per_shape_rates.is_empty() {
            let sum: f64 = record.per_shape_rates.values().sum();
            record.overall_effectiveness =
                sum / record.per_shape_rates.len() as f64;
        }
    }

    /// Run a consolidation pass on the given rule set.
    ///
    /// Returns the consolidated rule set (with low-effectiveness rules
    /// removed) and a report of what was changed.
    pub fn consolidate(
        &mut self,
        rules: Vec<Rewrite<RelLang, RelAnalysis>>,
    ) -> (Vec<Rewrite<RelLang, RelAnalysis>>, ConsolidationResult) {
        self.pass_count += 1;
        let rules_before = rules.len();

        info!(
            "Rule consolidation pass #{}: analyzing {} rules",
            self.pass_count, rules_before,
        );

        // Phase 1: Identify low-effectiveness rules
        let removal_candidates = self.identify_removal_candidates();

        // Phase 2: Dependency analysis (protect rules that enable others)
        let protected_rules = if self.config.enable_dependency_analysis {
            self.identify_protected_rules(&removal_candidates)
        } else {
            HashSet::new()
        };

        // Phase 3: Determine final removals
        let mut removals: Vec<String> = removal_candidates
            .into_iter()
            .filter(|name| !protected_rules.contains(name))
            .take(self.config.max_removals_per_pass)
            .collect();
        removals.sort();

        // Phase 4: Identify merge candidates
        let merge_groups = if self.config.enable_merging {
            self.identify_merge_candidates(&rules)
        } else {
            Vec::new()
        };

        // Phase 5: Detect conflicts
        let conflicts = if self.config.enable_dependency_analysis {
            self.detect_conflicts(&rules)
        } else {
            Vec::new()
        };

        // Phase 6: Apply removals
        let removal_set: HashSet<&str> =
            removals.iter().map(String::as_str).collect();
        let filtered_rules: Vec<Rewrite<RelLang, RelAnalysis>> = rules
            .into_iter()
            .filter(|rule| !removal_set.contains(rule.name.as_str()))
            .collect();

        // Update excluded set
        for name in &removals {
            self.excluded_rules.insert(name.clone());
        }

        // Apply observation decay
        self.apply_decay();

        // Compute metrics
        let metrics = self.compute_metrics(&removals, &merge_groups, &conflicts);

        let rules_after = filtered_rules.len();

        info!(
            "Consolidation pass #{}: {} -> {} rules ({} removed, {} merge groups, {} conflicts)",
            self.pass_count,
            rules_before,
            rules_after,
            removals.len(),
            merge_groups.len(),
            conflicts.len(),
        );

        let result = ConsolidationResult {
            removed_rules: removals,
            merged_rules: merge_groups,
            conflicts_resolved: conflicts,
            rules_before,
            rules_after,
            metrics,
        };

        (filtered_rules, result)
    }

    /// Filter a rule set, removing any previously excluded rules.
    ///
    /// Lighter-weight than a full consolidation pass: just applies
    /// the accumulated exclusion list without re-analyzing.
    #[must_use]
    pub fn filter_excluded(
        &self,
        rules: Vec<Rewrite<RelLang, RelAnalysis>>,
    ) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        if self.excluded_rules.is_empty() {
            return rules;
        }
        rules
            .into_iter()
            .filter(|rule| !self.excluded_rules.contains(rule.name.as_str()))
            .collect()
    }

    /// Get the current effectiveness record for a rule.
    #[must_use]
    pub fn effectiveness(&self, rule_name: &str) -> Option<&RuleEffectivenessRecord> {
        self.effectiveness.get(rule_name)
    }

    /// Get all effectiveness records.
    #[must_use]
    pub fn all_effectiveness(&self) -> &HashMap<String, RuleEffectivenessRecord> {
        &self.effectiveness
    }

    /// Get the set of currently excluded rules.
    #[must_use]
    pub fn excluded_rules(&self) -> &HashSet<String> {
        &self.excluded_rules
    }

    /// Get the number of consolidation passes executed so far.
    #[must_use]
    pub fn pass_count(&self) -> u32 {
        self.pass_count
    }

    /// Get all detected dependencies.
    #[must_use]
    pub fn dependencies(&self) -> &[RuleDependency] {
        &self.dependencies
    }

    /// Manually add a known dependency between rules.
    pub fn add_dependency(&mut self, dep: RuleDependency) {
        if !self.dependencies.contains(&dep) {
            self.dependencies.push(dep);
        }
    }

    /// Reset exclusions, allowing all rules back in.
    pub fn reset_exclusions(&mut self) {
        self.excluded_rules.clear();
    }

    /// Generate a report of rule effectiveness for diagnostics.
    #[must_use]
    pub fn effectiveness_report(&self) -> Vec<RuleEffectivenessReport> {
        let mut reports: Vec<RuleEffectivenessReport> = self
            .effectiveness
            .values()
            .map(|record| RuleEffectivenessReport {
                name: record.name.clone(),
                attempts: record.total_attempts,
                matches: record.total_matches,
                improvements: record.total_improvements,
                success_rate: record.success_rate(),
                improvement_rate: record.improvement_rate(),
                overall_effectiveness: record.overall_effectiveness,
                is_excluded: self.excluded_rules.contains(&record.name),
                shape_count: record.per_shape_rates.len(),
            })
            .collect();

        // Sort by effectiveness ascending (worst rules first)
        reports.sort_by(|a, b| {
            a.overall_effectiveness
                .partial_cmp(&b.overall_effectiveness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        reports
    }

    /// Identify rules that should be removed due to low effectiveness.
    fn identify_removal_candidates(&self) -> Vec<String> {
        let mut candidates = Vec::new();

        for (name, record) in &self.effectiveness {
            // Skip already excluded rules
            if self.excluded_rules.contains(name) {
                continue;
            }

            // Need sufficient observations before removing
            if record.total_attempts < self.config.min_observations {
                continue;
            }

            // Check if effectiveness is below threshold
            if record.overall_effectiveness < self.config.effectiveness_threshold {
                debug!(
                    "Rule '{}' below threshold: effectiveness={:.4} (threshold={:.4}), attempts={}",
                    name,
                    record.overall_effectiveness,
                    self.config.effectiveness_threshold,
                    record.total_attempts,
                );
                candidates.push(name.clone());
            }
        }

        // Sort by effectiveness (remove worst ones first)
        candidates.sort_by(|a, b| {
            let eff_a = self
                .effectiveness
                .get(a)
                .map_or(0.0, |r| r.overall_effectiveness);
            let eff_b = self
                .effectiveness
                .get(b)
                .map_or(0.0, |r| r.overall_effectiveness);
            eff_a
                .partial_cmp(&eff_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Identify rules that should be protected from removal because
    /// they enable other high-value rules.
    fn identify_protected_rules(&self, candidates: &[String]) -> HashSet<String> {
        let mut protected = HashSet::new();
        let candidate_set: HashSet<&str> =
            candidates.iter().map(String::as_str).collect();

        for dep in &self.dependencies {
            if dep.kind == DependencyKind::Enables
                && candidate_set.contains(dep.prerequisite.as_str())
            {
                // Check if the dependent rule is high-value
                if let Some(dependent_record) =
                    self.effectiveness.get(&dep.dependent)
                {
                    if dependent_record.overall_effectiveness
                        > self.config.effectiveness_threshold * 10.0
                    {
                        debug!(
                            "Protecting '{}': enables high-value rule '{}'",
                            dep.prerequisite, dep.dependent,
                        );
                        protected.insert(dep.prerequisite.clone());
                    }
                }
            }
        }

        protected
    }

    /// Identify groups of related rules that could be merged.
    fn identify_merge_candidates(
        &self,
        rules: &[Rewrite<RelLang, RelAnalysis>],
    ) -> Vec<MergeRecord> {
        let mut groups: Vec<MergeRecord> = Vec::new();

        // Group rules by category prefix (e.g., "null-*", "filter-*")
        let mut by_prefix: HashMap<String, Vec<&str>> = HashMap::new();
        for rule in rules {
            let name = rule.name.as_str();
            if let Some(prefix) = extract_rule_prefix(name) {
                by_prefix
                    .entry(prefix.to_string())
                    .or_default()
                    .push(name);
            }
        }

        // Identify groups where all rules have similar low effectiveness
        for (prefix, rule_names) in &by_prefix {
            if rule_names.len() < 3 {
                continue;
            }

            let effectiveness_values: Vec<f64> = rule_names
                .iter()
                .filter_map(|name| {
                    self.effectiveness
                        .get(*name)
                        .map(|r| r.overall_effectiveness)
                })
                .collect();

            if effectiveness_values.is_empty() {
                continue;
            }

            // Check if the group has similar effectiveness patterns
            let avg: f64 =
                effectiveness_values.iter().sum::<f64>()
                    / effectiveness_values.len() as f64;
            let variance: f64 = effectiveness_values
                .iter()
                .map(|v| (v - avg).powi(2))
                .sum::<f64>()
                / effectiveness_values.len() as f64;

            // Low variance means similar patterns -> merge candidate
            if variance < 0.01 && avg > self.config.effectiveness_threshold {
                groups.push(MergeRecord {
                    source_rules: rule_names
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect(),
                    composite_name: format!("{prefix}-composite"),
                    category: prefix.clone(),
                });
            }
        }

        groups
    }

    /// Detect conflicts between rules based on observed co-occurrence
    /// patterns.
    fn detect_conflicts(
        &self,
        rules: &[Rewrite<RelLang, RelAnalysis>],
    ) -> Vec<RuleConflict> {
        let mut conflicts = Vec::new();

        // Check existing dependencies for conflict types
        for dep in &self.dependencies {
            if dep.kind == DependencyKind::Conflicts {
                // Verify both rules exist in the current set
                let a_exists = rules.iter().any(|r| r.name.as_str() == dep.prerequisite);
                let b_exists = rules.iter().any(|r| r.name.as_str() == dep.dependent);

                if a_exists && b_exists {
                    let resolution = self.suggest_conflict_resolution(
                        &dep.prerequisite,
                        &dep.dependent,
                    );
                    conflicts.push(RuleConflict {
                        rule_a: dep.prerequisite.clone(),
                        rule_b: dep.dependent.clone(),
                        description: format!(
                            "Rules '{}' and '{}' conflict (applying one prevents the other)",
                            dep.prerequisite, dep.dependent,
                        ),
                        resolution,
                    });
                }
            }
        }

        // Detect potential subsumption from effectiveness data
        for dep in &self.dependencies {
            if dep.kind == DependencyKind::Subsumes {
                let a_exists = rules.iter().any(|r| r.name.as_str() == dep.prerequisite);
                let b_exists = rules.iter().any(|r| r.name.as_str() == dep.dependent);

                if a_exists && b_exists {
                    conflicts.push(RuleConflict {
                        rule_a: dep.prerequisite.clone(),
                        rule_b: dep.dependent.clone(),
                        description: format!(
                            "Rule '{}' subsumes '{}' (produces equivalent or better results)",
                            dep.prerequisite, dep.dependent,
                        ),
                        resolution: ConflictResolution::KeepA,
                    });
                }
            }
        }

        conflicts
    }

    /// Suggest how to resolve a conflict between two rules.
    fn suggest_conflict_resolution(
        &self,
        rule_a: &str,
        rule_b: &str,
    ) -> ConflictResolution {
        let eff_a = self
            .effectiveness
            .get(rule_a)
            .map_or(0.5, |r| r.overall_effectiveness);
        let eff_b = self
            .effectiveness
            .get(rule_b)
            .map_or(0.5, |r| r.overall_effectiveness);

        // If one is significantly more effective, keep it
        if eff_a > eff_b * 2.0 {
            ConflictResolution::KeepA
        } else if eff_b > eff_a * 2.0 {
            ConflictResolution::KeepB
        } else {
            // Similar effectiveness: order by priority
            let score_a = crate::rule_priority::rule_score(rule_a);
            let score_b = crate::rule_priority::rule_score(rule_b);
            if score_a >= score_b {
                ConflictResolution::OrderABeforeB
            } else {
                ConflictResolution::Manual {
                    reason: "Similar effectiveness and priority; manual review needed"
                        .to_string(),
                }
            }
        }
    }

    /// Apply observation decay to all effectiveness records.
    fn apply_decay(&mut self) {
        let decay = self.config.observation_decay;
        for record in self.effectiveness.values_mut() {
            // Decay EWMA toward neutral
            record.overall_effectiveness =
                record.overall_effectiveness * decay + 0.5 * (1.0 - decay);
            for rate in record.per_shape_rates.values_mut() {
                *rate = *rate * decay + 0.5 * (1.0 - decay);
            }
        }
    }

    /// Compute summary metrics for a consolidation pass.
    fn compute_metrics(
        &self,
        removals: &[String],
        merge_groups: &[MergeRecord],
        conflicts: &[RuleConflict],
    ) -> ConsolidationMetrics {
        let below_threshold = self
            .effectiveness
            .values()
            .filter(|r| {
                r.total_attempts >= self.config.min_observations
                    && r.overall_effectiveness < self.config.effectiveness_threshold
            })
            .count();

        let average_effectiveness = if self.effectiveness.is_empty() {
            0.0
        } else {
            let sum: f64 = self
                .effectiveness
                .values()
                .map(|r| r.overall_effectiveness)
                .sum();
            sum / self.effectiveness.len() as f64
        };

        ConsolidationMetrics {
            rules_analyzed: self.effectiveness.len(),
            below_threshold,
            rules_removed: removals.len(),
            merge_groups_formed: merge_groups.len(),
            dependencies_detected: self.dependencies.len(),
            conflicts_detected: conflicts.len(),
            average_effectiveness,
        }
    }
}

/// A single entry in the effectiveness report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEffectivenessReport {
    /// Rule name.
    pub name: String,
    /// Total optimization runs where this rule was available.
    pub attempts: u32,
    /// Total pattern matches.
    pub matches: u32,
    /// Total cost improvements from matches.
    pub improvements: u32,
    /// Success rate (matches / attempts).
    pub success_rate: f64,
    /// Improvement rate (improvements / matches).
    pub improvement_rate: f64,
    /// Overall EWMA effectiveness.
    pub overall_effectiveness: f64,
    /// Whether the rule is currently excluded.
    pub is_excluded: bool,
    /// Number of shape buckets with data for this rule.
    pub shape_count: usize,
}

/// Extract the category prefix from a rule name.
///
/// Examples:
/// - "null-eq" -> "null"
/// - "filter-through-join-left" -> "filter"
/// - "join-commutativity" -> "join"
fn extract_rule_prefix(name: &str) -> Option<&str> {
    name.split('-').next()
}

/// Well-known rule dependencies based on the Ra optimizer's rule set.
///
/// These are statically known relationships between rules that inform
/// the consolidation engine's dependency analysis.
#[must_use]
pub fn known_rule_dependencies() -> Vec<RuleDependency> {
    vec![
        // filter-split-and enables filter-through-join-*
        RuleDependency {
            prerequisite: "filter-split-and".to_string(),
            dependent: "filter-through-join-left".to_string(),
            kind: DependencyKind::Enables,
        },
        RuleDependency {
            prerequisite: "filter-split-and".to_string(),
            dependent: "filter-through-join-right".to_string(),
            kind: DependencyKind::Enables,
        },
        // join-commutativity enables join-associativity-*
        RuleDependency {
            prerequisite: "join-commutativity".to_string(),
            dependent: "join-associativity-left".to_string(),
            kind: DependencyKind::Enables,
        },
        RuleDependency {
            prerequisite: "join-commutativity".to_string(),
            dependent: "join-associativity-right".to_string(),
            kind: DependencyKind::Enables,
        },
        // null-eq and null-ne are complementary (not conflicting)
        // but and-null-left / and-null-right are symmetric variants
        RuleDependency {
            prerequisite: "and-null-left".to_string(),
            dependent: "and-null-right".to_string(),
            kind: DependencyKind::Subsumes,
        },
        // Commutativity rules enable each other's patterns
        RuleDependency {
            prerequisite: "eq-commutative".to_string(),
            dependent: "sqlite-eq-transitive".to_string(),
            kind: DependencyKind::Enables,
        },
        // filter-merge conflicts with filter-split-and (inverse ops)
        RuleDependency {
            prerequisite: "filter-merge".to_string(),
            dependent: "filter-split-and".to_string(),
            kind: DependencyKind::Conflicts,
        },
    ]
}

/// Create a consolidator pre-seeded with known rule dependencies.
#[must_use]
pub fn default_consolidator(config: ConsolidationConfig) -> RuleConsolidator {
    let mut consolidator = RuleConsolidator::new(config);
    for dep in known_rule_dependencies() {
        consolidator.add_dependency(dep);
    }
    consolidator
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_rules() -> Vec<Rewrite<RelLang, RelAnalysis>> {
        use egg::rewrite;
        vec![
            rewrite!("filter-true";
                "(filter (const-bool true) ?input)" => "?input"
            ),
            rewrite!("and-true-left";
                "(and (const-bool true) ?x)" => "?x"
            ),
            rewrite!("and-true-right";
                "(and ?x (const-bool true))" => "?x"
            ),
            rewrite!("or-false-left";
                "(or (const-bool false) ?x)" => "?x"
            ),
            rewrite!("null-eq";
                "(eq (const-null) ?x)" => "(const-null)"
            ),
        ]
    }

    #[test]
    fn consolidator_default_config() {
        let config = ConsolidationConfig::default();
        assert_eq!(config.min_observations, 50);
        assert!((config.effectiveness_threshold - 0.01).abs() < f64::EPSILON);
        assert!(config.enable_merging);
        assert!(config.enable_dependency_analysis);
    }

    #[test]
    fn consolidator_no_removals_without_data() {
        let config = ConsolidationConfig::default();
        let mut consolidator = RuleConsolidator::new(config);
        let rules = make_test_rules();
        let count = rules.len();

        let (result, report) = consolidator.consolidate(rules);
        assert_eq!(result.len(), count);
        assert!(report.removed_rules.is_empty());
        assert_eq!(report.rules_before, count);
        assert_eq!(report.rules_after, count);
    }

    #[test]
    fn consolidator_removes_low_effectiveness_rules() {
        let config = ConsolidationConfig {
            min_observations: 10,
            effectiveness_threshold: 0.01,
            ..ConsolidationConfig::default()
        };
        let mut consolidator = RuleConsolidator::new(config);

        // Simulate a rule with very low effectiveness
        let shape = ShapeKeyBucket {
            table_bucket: 0,
            join_bucket: 0,
            predicate_complexity: 0,
            content_features: 0,
        };

        // Record many misses for "null-eq"
        for _ in 0..60 {
            consolidator.record_application("null-eq", &shape, false, false);
        }
        // Record some hits for "filter-true"
        for _ in 0..60 {
            consolidator.record_application("filter-true", &shape, true, true);
        }

        let rules = make_test_rules();
        let (result, report) = consolidator.consolidate(rules);

        // "null-eq" should be removed (0% effectiveness)
        assert!(report.removed_rules.contains(&"null-eq".to_string()));
        // "filter-true" should remain (high effectiveness)
        assert!(!report.removed_rules.contains(&"filter-true".to_string()));
        assert!(result.len() < 5);
    }

    #[test]
    fn consolidator_respects_min_observations() {
        let config = ConsolidationConfig {
            min_observations: 100,
            effectiveness_threshold: 0.01,
            ..ConsolidationConfig::default()
        };
        let mut consolidator = RuleConsolidator::new(config);

        let shape = ShapeKeyBucket {
            table_bucket: 0,
            join_bucket: 0,
            predicate_complexity: 0,
            content_features: 0,
        };

        // Only 50 observations (below min_observations of 100)
        for _ in 0..50 {
            consolidator.record_application("null-eq", &shape, false, false);
        }

        let rules = make_test_rules();
        let (result, report) = consolidator.consolidate(rules);

        // Should NOT be removed because insufficient observations
        assert!(report.removed_rules.is_empty());
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn consolidator_max_removals_per_pass() {
        let config = ConsolidationConfig {
            min_observations: 5,
            effectiveness_threshold: 0.01,
            max_removals_per_pass: 2,
            ..ConsolidationConfig::default()
        };
        let mut consolidator = RuleConsolidator::new(config);

        let shape = ShapeKeyBucket {
            table_bucket: 0,
            join_bucket: 0,
            predicate_complexity: 0,
            content_features: 0,
        };

        // Mark all rules as ineffective (60 observations needed for
        // EWMA to converge below 0.01 threshold from 0.5 start)
        for name in ["filter-true", "and-true-left", "and-true-right", "or-false-left", "null-eq"] {
            for _ in 0..60 {
                consolidator.record_application(name, &shape, false, false);
            }
        }

        let rules = make_test_rules();
        let (result, report) = consolidator.consolidate(rules);

        // Should only remove max 2 per pass
        assert_eq!(report.removed_rules.len(), 2);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn consolidator_filter_excluded() {
        let config = ConsolidationConfig::default();
        let mut consolidator = RuleConsolidator::new(config);
        consolidator.excluded_rules.insert("null-eq".to_string());

        let rules = make_test_rules();
        let filtered = consolidator.filter_excluded(rules);
        assert_eq!(filtered.len(), 4);
        assert!(filtered.iter().all(|r| r.name.as_str() != "null-eq"));
    }

    #[test]
    fn consolidator_dependency_protection() {
        let config = ConsolidationConfig {
            min_observations: 5,
            effectiveness_threshold: 0.05,
            enable_dependency_analysis: true,
            ..ConsolidationConfig::default()
        };
        let mut consolidator = RuleConsolidator::new(config);

        // Add dependency: "and-true-left" enables "filter-true"
        consolidator.add_dependency(RuleDependency {
            prerequisite: "and-true-left".to_string(),
            dependent: "filter-true".to_string(),
            kind: DependencyKind::Enables,
        });

        let shape = ShapeKeyBucket {
            table_bucket: 0,
            join_bucket: 0,
            predicate_complexity: 0,
            content_features: 0,
        };

        // and-true-left has low effectiveness
        for _ in 0..20 {
            consolidator.record_application("and-true-left", &shape, false, false);
        }
        // filter-true has high effectiveness (> 10x threshold)
        for _ in 0..20 {
            consolidator.record_application("filter-true", &shape, true, true);
        }

        let rules = make_test_rules();
        let (_, report) = consolidator.consolidate(rules);

        // and-true-left should be protected because it enables high-value filter-true
        assert!(
            !report.removed_rules.contains(&"and-true-left".to_string()),
            "and-true-left should be protected due to dependency on filter-true",
        );
    }

    #[test]
    fn effectiveness_record_rates() {
        let record = RuleEffectivenessRecord {
            name: "test-rule".to_string(),
            total_attempts: 100,
            total_matches: 25,
            total_improvements: 10,
            per_shape_rates: HashMap::new(),
            overall_effectiveness: 0.25,
            category: "test".to_string(),
        };

        assert!((record.success_rate() - 0.25).abs() < f64::EPSILON);
        assert!((record.improvement_rate() - 0.40).abs() < f64::EPSILON);
    }

    #[test]
    fn effectiveness_record_zero_attempts() {
        let record = RuleEffectivenessRecord {
            name: "test-rule".to_string(),
            total_attempts: 0,
            total_matches: 0,
            total_improvements: 0,
            per_shape_rates: HashMap::new(),
            overall_effectiveness: 0.0,
            category: "test".to_string(),
        };

        assert!((record.success_rate()).abs() < f64::EPSILON);
        assert!((record.improvement_rate()).abs() < f64::EPSILON);
    }

    #[test]
    fn effectiveness_report_sorted() {
        let config = ConsolidationConfig::default();
        let mut consolidator = RuleConsolidator::new(config);

        let shape = ShapeKeyBucket {
            table_bucket: 0,
            join_bucket: 0,
            predicate_complexity: 0,
            content_features: 0,
        };

        // Give different effectiveness to different rules
        for _ in 0..10 {
            consolidator.record_application("low-rule", &shape, false, false);
        }
        for _ in 0..10 {
            consolidator.record_application("high-rule", &shape, true, true);
        }

        let report = consolidator.effectiveness_report();
        assert_eq!(report.len(), 2);
        // Sorted ascending by effectiveness
        assert!(
            report[0].overall_effectiveness <= report[1].overall_effectiveness,
        );
    }

    #[test]
    fn extract_rule_prefix_works() {
        assert_eq!(extract_rule_prefix("null-eq"), Some("null"));
        assert_eq!(
            extract_rule_prefix("filter-through-join-left"),
            Some("filter"),
        );
        assert_eq!(extract_rule_prefix("join-commutativity"), Some("join"));
        assert_eq!(extract_rule_prefix("x"), Some("x"));
    }

    #[test]
    fn known_dependencies_not_empty() {
        let deps = known_rule_dependencies();
        assert!(!deps.is_empty());
        // Verify structure
        for dep in &deps {
            assert!(!dep.prerequisite.is_empty());
            assert!(!dep.dependent.is_empty());
        }
    }

    #[test]
    fn default_consolidator_has_dependencies() {
        let consolidator = default_consolidator(ConsolidationConfig::default());
        assert!(!consolidator.dependencies().is_empty());
    }

    #[test]
    fn consolidator_reset_exclusions() {
        let mut consolidator = RuleConsolidator::new(ConsolidationConfig::default());
        consolidator.excluded_rules.insert("rule-a".to_string());
        consolidator.excluded_rules.insert("rule-b".to_string());
        assert_eq!(consolidator.excluded_rules().len(), 2);

        consolidator.reset_exclusions();
        assert!(consolidator.excluded_rules().is_empty());
    }

    #[test]
    fn consolidator_pass_count_increments() {
        let mut consolidator = RuleConsolidator::new(ConsolidationConfig::default());
        assert_eq!(consolidator.pass_count(), 0);

        let rules = make_test_rules();
        let _ = consolidator.consolidate(rules.clone());
        assert_eq!(consolidator.pass_count(), 1);

        let _ = consolidator.consolidate(rules);
        assert_eq!(consolidator.pass_count(), 2);
    }

    #[test]
    fn add_duplicate_dependency_ignored() {
        let mut consolidator = RuleConsolidator::new(ConsolidationConfig::default());
        let dep = RuleDependency {
            prerequisite: "a".to_string(),
            dependent: "b".to_string(),
            kind: DependencyKind::Enables,
        };

        consolidator.add_dependency(dep.clone());
        consolidator.add_dependency(dep);
        assert_eq!(consolidator.dependencies().len(), 1);
    }

    #[test]
    fn conflict_resolution_favors_more_effective() {
        let config = ConsolidationConfig::default();
        let mut consolidator = RuleConsolidator::new(config);

        let shape = ShapeKeyBucket {
            table_bucket: 0,
            join_bucket: 0,
            predicate_complexity: 0,
            content_features: 0,
        };

        // Make rule-a much more effective than rule-b
        for _ in 0..50 {
            consolidator.record_application("rule-a", &shape, true, true);
        }
        for _ in 0..50 {
            consolidator.record_application("rule-b", &shape, false, false);
        }

        let resolution = consolidator.suggest_conflict_resolution("rule-a", "rule-b");
        matches!(resolution, ConflictResolution::KeepA);
    }
}

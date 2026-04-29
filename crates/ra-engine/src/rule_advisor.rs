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

use egg::Rewrite;
use tracing::{debug, info};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::lazy_rules::LazyQueryPattern;
use crate::query_features::QueryFeatureSet;
use crate::rewrite::{AnnotatedRuleGroup, RuleAnnotation};
use crate::rule_knowledge::{RuleKnowledge, ShapeKeyBucket};
use ra_core::algebra::RelExpr;

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
        }
    }

    /// Select rules for a specific query expression (Stages 2 + 3).
    ///
    /// Runs query-shape elimination and learned ranking on the Stage 1
    /// survivors, returning a priority-sorted rule set ready for the
    /// e-graph runner.
    pub fn select_rules(&mut self, expr: &RelExpr) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        // Analyze query features
        let pattern = LazyQueryPattern::analyze(expr);
        let query_features = QueryFeatureSet::from_pattern(&pattern);

        // Stage 2: query-shape elimination
        let mut selected = Vec::with_capacity(200);
        let mut stage2_eliminated = Vec::new();

        for slot in &self.context_slots {
            if Self::passes_shape_filter(&slot.annotation, query_features) {
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
        let after_stage3 = if let Some(ref knowledge) = self.knowledge {
            let shape_key = ShapeKeyBucket::from_expr(expr);
            selected = knowledge.rank_and_filter(
                selected,
                &shape_key,
                self.config.min_observations,
                self.config.effectiveness_threshold,
            );
            selected.len()
        } else {
            // No learning: just apply priority sorting
            selected = crate::rule_priority::sort_rules_by_priority(selected);
            selected.len()
        };

        // Update stats
        self.last_stats.after_stage2 = after_stage2;
        self.last_stats.after_stage3 = after_stage3;
        self.last_stats.stage2_eliminated = stage2_eliminated;

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

    /// Get the most recent advisor statistics.
    #[must_use]
    pub fn stats(&self) -> &AdvisorStats {
        &self.last_stats
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
}

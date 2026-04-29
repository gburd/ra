//! Persistent rule effectiveness knowledge store.
//!
//! Tracks which rewrite rules fire (and improve cost) for different
//! query shape buckets. Uses exponentially weighted moving averages
//! (EWMA) to smooth observations over time, enabling the rule advisor
//! to deprioritize chronically ineffective rules.
//!
//! Persistence format: bincode for speed, with atomic write-rename to
//! avoid corruption. Typical size is ~100 KB.

use std::collections::HashMap;
use std::path::Path;

use egg::Rewrite;
use serde::{Deserialize, Serialize};

use crate::analysis::RelAnalysis;
use crate::egraph::RelLang;
use crate::lazy_rules::LazyQueryPattern;
use crate::query_features::QueryFeatureSet;
use ra_core::algebra::RelExpr;

/// Shape bucket for grouping structurally similar queries.
///
/// Queries with the same bucket produce the same learned-ranking
/// context, so rules that never fire for 4-table joins with simple
/// predicates are deprioritized across all such queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShapeKeyBucket {
    /// Table count bucket: 0-1, 2-3, 4-6, 7+
    pub table_bucket: u8,
    /// Join count bucket: 0, 1-2, 3-5, 6+
    pub join_bucket: u8,
    /// Predicate complexity: 0=low, 1=medium, 2=high
    pub predicate_complexity: u8,
    /// Content feature flags (serialized as u32)
    pub content_features: u32,
}

impl ShapeKeyBucket {
    /// Build a shape key from a `RelExpr`.
    #[must_use]
    pub fn from_expr(expr: &RelExpr) -> Self {
        let pattern = LazyQueryPattern::analyze(expr);
        let features = QueryFeatureSet::from_pattern(&pattern);

        let table_bucket = match pattern.table_count {
            0..=1 => 0,
            2..=3 => 1,
            4..=6 => 2,
            _ => 3,
        };

        let join_bucket = match pattern.join_depth {
            0 => 0,
            1..=2 => 1,
            3..=5 => 2,
            _ => 3,
        };

        // Heuristic: predicate complexity based on feature count
        let feature_count = features.count();
        let predicate_complexity = match feature_count {
            0..=2 => 0,
            3..=5 => 1,
            _ => 2,
        };

        Self {
            table_bucket,
            join_bucket,
            predicate_complexity,
            content_features: features.bits(),
        }
    }
}

/// Effectiveness entry for a single rule within a shape bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEffectivenessEntry {
    /// Number of optimization runs where this rule was available.
    pub attempts: u32,
    /// Number of times the rule pattern matched.
    pub matches: u32,
    /// Number of times matching led to cost improvement.
    pub improvements: u32,
    /// Exponentially weighted moving average of effectiveness.
    /// Effectiveness = matches / attempts (or improvements / matches).
    pub ewma_effectiveness: f64,
}

impl Default for RuleEffectivenessEntry {
    fn default() -> Self {
        Self {
            attempts: 0,
            matches: 0,
            improvements: 0,
            ewma_effectiveness: 0.5, // Start with neutral prior
        }
    }
}

/// Persistent knowledge store for rule effectiveness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleKnowledge {
    /// Per-shape-bucket effectiveness data.
    ///
    /// Serialized as a Vec of (key, value) pairs because JSON requires
    /// string keys and `ShapeKeyBucket` is a struct.
    #[serde(
        serialize_with = "serialize_entries",
        deserialize_with = "deserialize_entries"
    )]
    entries: HashMap<ShapeKeyBucket, HashMap<String, RuleEffectivenessEntry>>,
    /// Global (shape-independent) effectiveness data.
    global: HashMap<String, RuleEffectivenessEntry>,
    /// EWMA smoothing factor.
    #[serde(default = "default_alpha")]
    alpha: f64,
}

fn serialize_entries<S>(
    entries: &HashMap<ShapeKeyBucket, HashMap<String, RuleEffectivenessEntry>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let vec: Vec<_> = entries.iter().collect();
    vec.serialize(serializer)
}

fn deserialize_entries<'de, D>(
    deserializer: D,
) -> Result<HashMap<ShapeKeyBucket, HashMap<String, RuleEffectivenessEntry>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let vec: Vec<(ShapeKeyBucket, HashMap<String, RuleEffectivenessEntry>)> =
        Vec::deserialize(deserializer)?;
    Ok(vec.into_iter().collect())
}

fn default_alpha() -> f64 {
    0.1
}

impl RuleKnowledge {
    /// Create a new empty knowledge store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            global: HashMap::new(),
            alpha: default_alpha(),
        }
    }

    /// Create with a custom EWMA alpha.
    #[must_use]
    pub fn with_alpha(alpha: f64) -> Self {
        Self {
            entries: HashMap::new(),
            global: HashMap::new(),
            alpha,
        }
    }

    /// Number of shape buckets with recorded data.
    #[must_use]
    pub fn bucket_count(&self) -> usize {
        self.entries.len()
    }

    /// Record that a rule was applied (matched) for a given shape.
    pub fn record_application(&mut self, shape: &ShapeKeyBucket, rule_name: &str) {
        // Update per-shape entry
        let bucket = self.entries.entry(shape.clone()).or_default();
        let entry = bucket.entry(rule_name.to_string()).or_default();
        entry.attempts += 1;
        entry.matches += 1;
        let observation = 1.0; // Rule fired
        entry.ewma_effectiveness =
            self.alpha * observation + (1.0 - self.alpha) * entry.ewma_effectiveness;

        // Update global entry
        let global = self.global.entry(rule_name.to_string()).or_default();
        global.attempts += 1;
        global.matches += 1;
        global.ewma_effectiveness =
            self.alpha * observation + (1.0 - self.alpha) * global.ewma_effectiveness;
    }

    /// Record that a rule was available but did NOT fire.
    pub fn record_miss(&mut self, shape: &ShapeKeyBucket, rule_name: &str) {
        let bucket = self.entries.entry(shape.clone()).or_default();
        let entry = bucket.entry(rule_name.to_string()).or_default();
        entry.attempts += 1;
        let observation = 0.0;
        entry.ewma_effectiveness =
            self.alpha * observation + (1.0 - self.alpha) * entry.ewma_effectiveness;

        let global = self.global.entry(rule_name.to_string()).or_default();
        global.attempts += 1;
        global.ewma_effectiveness =
            self.alpha * observation + (1.0 - self.alpha) * global.ewma_effectiveness;
    }

    /// Rank and filter rules based on learned effectiveness.
    ///
    /// Rules with enough observations and effectiveness below the
    /// threshold are moved to the end (deprioritized). All other
    /// rules are sorted by priority with effectiveness as a tiebreaker.
    #[must_use]
    pub fn rank_and_filter(
        &self,
        rules: Vec<Rewrite<RelLang, RelAnalysis>>,
        shape: &ShapeKeyBucket,
        min_observations: u32,
        effectiveness_threshold: f64,
    ) -> Vec<Rewrite<RelLang, RelAnalysis>> {
        let bucket_data = self.entries.get(shape);

        let priorities = crate::rule_priority::default_rule_priorities();
        let default_score = crate::rule_priority::compute_priority(
            crate::rule_priority::DEFAULT_COMPLEXITY,
            crate::rule_priority::DEFAULT_BENEFIT,
        );

        let mut scored: Vec<(f64, bool, usize, Rewrite<RelLang, RelAnalysis>)> = rules
            .into_iter()
            .enumerate()
            .map(|(idx, rule)| {
                let name = rule.name.as_str();

                // Base priority score
                let base_score = priorities.get(name).map_or(default_score, |&(c, b)| {
                    crate::rule_priority::compute_priority(c, b)
                });

                // Learned effectiveness boost/penalty
                let (effectiveness, should_deprioritize) = if let Some(data) = bucket_data {
                    if let Some(entry) = data.get(name) {
                        let deprioritize = entry.attempts >= min_observations
                            && entry.ewma_effectiveness < effectiveness_threshold;
                        (entry.ewma_effectiveness, deprioritize)
                    } else {
                        // No data for this rule in this bucket:
                        // check global
                        self.global_effectiveness(name, min_observations, effectiveness_threshold)
                    }
                } else {
                    // No bucket data at all: use global
                    self.global_effectiveness(name, min_observations, effectiveness_threshold)
                };

                // Combined score: base priority * (0.5 + 0.5 * effectiveness)
                let combined = base_score * (0.5 + 0.5 * effectiveness);

                (combined, should_deprioritize, idx, rule)
            })
            .collect();

        // Sort: non-deprioritized first (by combined score desc),
        // then deprioritized (by combined score desc).
        scored.sort_by(|a, b| {
            a.1.cmp(&b.1) // false (keep) before true (deprioritize)
                .then(b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.2.cmp(&b.2))
        });

        scored.into_iter().map(|(_, _, _, rule)| rule).collect()
    }

    /// Get global effectiveness data for a rule.
    fn global_effectiveness(
        &self,
        name: &str,
        min_observations: u32,
        threshold: f64,
    ) -> (f64, bool) {
        if let Some(entry) = self.global.get(name) {
            let deprioritize =
                entry.attempts >= min_observations && entry.ewma_effectiveness < threshold;
            (entry.ewma_effectiveness, deprioritize)
        } else {
            // No data at all: neutral effectiveness, don't deprioritize
            (0.5, false)
        }
    }

    /// Merge another knowledge store into this one.
    ///
    /// Uses weighted averaging by observation count so that stores
    /// with more data contribute proportionally more.
    pub fn merge(&mut self, other: &Self) {
        for (shape, other_bucket) in &other.entries {
            let bucket = self.entries.entry(shape.clone()).or_default();
            for (name, other_entry) in other_bucket {
                let entry = bucket.entry(name.clone()).or_default();
                merge_entries(entry, other_entry);
            }
        }
        for (name, other_entry) in &other.global {
            let entry = self.global.entry(name.clone()).or_default();
            merge_entries(entry, other_entry);
        }
    }

    /// Load knowledge from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file doesn't exist or can't be decoded.
    pub fn load(path: &Path) -> Result<Self, anyhow::Error> {
        let data = std::fs::read_to_string(path)?;
        let knowledge: Self = serde_json::from_str(&data)?;
        Ok(knowledge)
    }

    /// Persist knowledge to a JSON file (atomic write-rename).
    ///
    /// # Errors
    ///
    /// Returns an error if the directory can't be created or the
    /// file can't be written.
    pub fn save(&self, path: &Path) -> Result<(), anyhow::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string(self)?;
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, &data)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

impl Default for RuleKnowledge {
    fn default() -> Self {
        Self::new()
    }
}

/// Weighted-average merge of two effectiveness entries.
fn merge_entries(dst: &mut RuleEffectivenessEntry, src: &RuleEffectivenessEntry) {
    let total = dst.attempts + src.attempts;
    if total == 0 {
        return;
    }
    let w_dst = f64::from(dst.attempts) / f64::from(total);
    let w_src = f64::from(src.attempts) / f64::from(total);

    dst.ewma_effectiveness = w_dst * dst.ewma_effectiveness + w_src * src.ewma_effectiveness;
    dst.attempts = total;
    dst.matches += src.matches;
    dst.improvements += src.improvements;
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn new_knowledge_is_empty() {
        let k = RuleKnowledge::new();
        assert_eq!(k.bucket_count(), 0);
    }

    #[test]
    fn record_and_query() {
        let mut k = RuleKnowledge::with_alpha(0.5);
        let shape = ShapeKeyBucket {
            table_bucket: 1,
            join_bucket: 1,
            predicate_complexity: 0,
            content_features: 0,
        };

        k.record_application(&shape, "filter-pushdown");
        k.record_application(&shape, "filter-pushdown");
        k.record_miss(&shape, "vector-search");

        let bucket = k.entries.get(&shape);
        assert!(bucket.is_some());
        let fp = &bucket.unwrap()["filter-pushdown"];
        assert_eq!(fp.attempts, 2);
        assert_eq!(fp.matches, 2);
        // EWMA after two hits with alpha=0.5:
        // start: 0.5 -> 0.5*1.0 + 0.5*0.5 = 0.75 -> 0.5*1.0 + 0.5*0.75 = 0.875
        assert!((fp.ewma_effectiveness - 0.875).abs() < 1e-6);

        let vs = &bucket.unwrap()["vector-search"];
        assert_eq!(vs.attempts, 1);
        assert_eq!(vs.matches, 0);
    }

    #[test]
    fn merge_knowledge() {
        let mut k1 = RuleKnowledge::new();
        let mut k2 = RuleKnowledge::new();
        let shape = ShapeKeyBucket {
            table_bucket: 0,
            join_bucket: 0,
            predicate_complexity: 0,
            content_features: 0,
        };

        k1.record_application(&shape, "rule-a");
        k2.record_application(&shape, "rule-a");
        k2.record_application(&shape, "rule-a");

        k1.merge(&k2);
        let entry = &k1.entries[&shape]["rule-a"];
        assert_eq!(entry.attempts, 3);
    }

    #[test]
    fn shape_key_from_simple_scan() {
        let expr = RelExpr::scan("t");
        let key = ShapeKeyBucket::from_expr(&expr);
        assert_eq!(key.table_bucket, 0);
        assert_eq!(key.join_bucket, 0);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut k = RuleKnowledge::new();
        let shape = ShapeKeyBucket {
            table_bucket: 2,
            join_bucket: 1,
            predicate_complexity: 1,
            content_features: 3,
        };
        k.record_application(&shape, "test-rule");

        let dir = std::env::temp_dir().join("ra-test-knowledge");
        let path = dir.join("test.bincode");
        k.save(&path).expect("save should succeed");

        let loaded = RuleKnowledge::load(&path).expect("load should succeed");
        assert_eq!(loaded.bucket_count(), 1);
        assert!(loaded.entries.contains_key(&shape));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}

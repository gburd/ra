//! [`PlanProvenance`] — observable per-query metadata describing
//! which inputs produced a plan.
//!
//! This is lesson (ii) of the GEQO comparison: `PostgreSQL` exposes
//! `geqo_seed` so that users who see a surprising plan can reproduce
//! it; Ra's plan is determined by a wider set of inputs (rules,
//! statistics, hardware profile, cost-model weights, route) but none
//! of them were observable until now. `PlanProvenance` packages all
//! those identifiers into a single struct that can be:
//!
//! - Attached to [`crate::OptimizationResult`] so any caller can
//!   inspect it.
//! - Rendered alongside `EXPLAIN` output (CLI and PG extension).
//! - Diffed between two plans to answer "what changed?".
//!
//! The struct is intentionally cheap to construct — every field is
//! either an integer hash, a `&'static str`, or a small fingerprint
//! type. All hashes are best-effort identifiers, not cryptographic
//! commitments; collisions are tolerable because the consumer is a
//! human inspecting a debug field, not a security boundary.

use serde::{Deserialize, Serialize};

use crate::genetic_fingerprint::QueryFingerprint;
use crate::speculative_router::OptRoute;

/// A bundle of identifiers describing which inputs produced a plan.
///
/// Field semantics:
///
/// - `fingerprint`: structural identity of the input query (tables,
///   predicate shape, aggregation signature). Two queries with the
///   same fingerprint differ only in literal values.
/// - `cost_model_id`: 16-hex-char prefix of a hash over the `BitNet`
///   cost-model weights, when a cost model is in use. `None` when
///   the optimizer was constructed without one.
/// - `stats_version`: monotonic counter from the statistics layer
///   when available, identifying which `pg_statistic` snapshot the
///   optimizer read.
/// - `hardware_profile_hash`: hash of the active `HardwareProfile`,
///   which feeds the cost function.
/// - `active_rule_set_hash`: stable hash of the sorted names of all
///   rules that were *available* during this optimization (after
///   advisor filtering, before saturation). Two plans produced by
///   different rule subsets will have different hashes here.
/// - `optimizer_version`: the `ra-engine` package version compiled
///   into the binary.
/// - `route`: which speculative-router branch was chosen.
/// - `termination_reason`: why saturation stopped (one of
///   `"iteration_limit"`, `"timeout"`, `"node_growth_budget"`,
///   `"application_budget"`, `"saturated"`, `"converged"`,
///   `"cost_stagnant"`, `"panic_in_apply"`, ...).
/// - `seed`: reserved for the GEQO-style genetic-algorithm fallback
///   proposed in RFC 0035; unused today.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanProvenance {
    /// Structural identity of the input query.
    pub fingerprint: QueryFingerprint,
    /// Short hash identifying the `BitNet` cost-model snapshot in
    /// effect during optimization. `None` means no cost model was
    /// loaded.
    pub cost_model_id: Option<String>,
    /// Monotonic stats-layer version, when known.
    pub stats_version: Option<u64>,
    /// Stable hash of the active hardware profile.
    pub hardware_profile_hash: u64,
    /// Stable hash of the sorted names of all rules available
    /// during this optimization.
    pub active_rule_set_hash: u64,
    /// `env!("CARGO_PKG_VERSION")` baked in at compile time.
    pub optimizer_version: &'static str,
    /// The speculative-router route taken for this query.
    pub route: OptRoute,
    /// Termination reason — one of a small set of `&'static str`
    /// values produced by the saturation loop.
    pub termination_reason: &'static str,
    /// Reserved for future GA-fallback (RFC 0035) seed value.
    pub seed: Option<u64>,
}

impl PlanProvenance {
    /// Build a provenance record from raw inputs. Most callers will
    /// use one of the higher-level constructors that knows how to
    /// hash a `HardwareProfile` or a `&[Rewrite<...>]` directly.
    #[must_use]
    pub fn new(
        fingerprint: QueryFingerprint,
        cost_model_id: Option<String>,
        stats_version: Option<u64>,
        hardware_profile_hash: u64,
        active_rule_set_hash: u64,
        route: OptRoute,
        termination_reason: &'static str,
    ) -> Self {
        Self {
            fingerprint,
            cost_model_id,
            stats_version,
            hardware_profile_hash,
            active_rule_set_hash,
            optimizer_version: env!("CARGO_PKG_VERSION"),
            route,
            termination_reason,
            seed: None,
        }
    }

    /// Compute a stable hash of a hardware profile.
    ///
    /// We hash a tuple of the most stable fields. Cache levels and
    /// memory totals can drift slightly during a run; we exclude
    /// them so two queries on the same machine produce the same
    /// hash.
    #[must_use]
    pub fn hash_hardware_profile(profile: &ra_hardware::HardwareProfile) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        profile.cpu_cores.hash(&mut h);
        profile.simd_width_bits.hash(&mut h);
        profile.gpu_available.hash(&mut h);
        profile.fpga_available.hash(&mut h);
        profile.numa_nodes.hash(&mut h);
        h.finish()
    }

    /// Compute a stable hash of the active rule set. Sorts rule
    /// names first so order-of-insertion does not affect the hash.
    #[must_use]
    pub fn hash_rule_names<I, S>(names: I) -> u64
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        use std::hash::{Hash, Hasher};
        let mut sorted: Vec<String> =
            names.into_iter().map(|s| s.as_ref().to_string()).collect();
        sorted.sort();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for name in &sorted {
            name.hash(&mut h);
        }
        h.finish()
    }

    /// Names of fields that differ between two provenance values.
    /// Useful when answering "this plan changed; what changed
    /// upstream?".
    #[must_use]
    pub fn diff(&self, other: &Self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.fingerprint != other.fingerprint {
            out.push("fingerprint");
        }
        if self.cost_model_id != other.cost_model_id {
            out.push("cost_model_id");
        }
        if self.stats_version != other.stats_version {
            out.push("stats_version");
        }
        if self.hardware_profile_hash != other.hardware_profile_hash {
            out.push("hardware_profile_hash");
        }
        if self.active_rule_set_hash != other.active_rule_set_hash {
            out.push("active_rule_set_hash");
        }
        if self.optimizer_version != other.optimizer_version {
            out.push("optimizer_version");
        }
        if self.route != other.route {
            out.push("route");
        }
        if self.termination_reason != other.termination_reason {
            out.push("termination_reason");
        }
        if self.seed != other.seed {
            out.push("seed");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::RelExpr;

    fn fp() -> QueryFingerprint {
        QueryFingerprint::from_rel_expr(&RelExpr::Scan {
            table: "t".into(),
            alias: None,
        })
    }

    #[test]
    fn diff_reports_changed_fields_only() {
        let a = PlanProvenance::new(fp(), None, None, 1, 1, OptRoute::Skip, "saturated");
        let mut b = a.clone();
        assert!(a.diff(&b).is_empty(), "identical inputs should produce no diff");

        b.hardware_profile_hash = 2;
        b.termination_reason = "timeout";
        let diff = a.diff(&b);
        assert!(diff.contains(&"hardware_profile_hash"));
        assert!(diff.contains(&"termination_reason"));
        assert_eq!(diff.len(), 2);
    }

    #[test]
    fn rule_name_hash_is_order_independent() {
        let a = PlanProvenance::hash_rule_names(["alpha", "beta", "gamma"]);
        let b = PlanProvenance::hash_rule_names(["gamma", "alpha", "beta"]);
        assert_eq!(a, b, "rule-name hash must not depend on input order");
    }

    #[test]
    fn rule_name_hash_distinguishes_different_sets() {
        let a = PlanProvenance::hash_rule_names(["alpha", "beta"]);
        let b = PlanProvenance::hash_rule_names(["alpha", "gamma"]);
        assert_ne!(a, b, "different rule sets must hash differently");
    }

    #[test]
    fn optimizer_version_matches_package() {
        let p = PlanProvenance::new(fp(), None, None, 0, 0, OptRoute::Skip, "saturated");
        assert_eq!(p.optimizer_version, env!("CARGO_PKG_VERSION"));
    }
}

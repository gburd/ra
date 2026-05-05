//! Multi-dimension weighted scoring for Ra vs. reference plan comparison.
//!
//! Produces a single [0, 1] composite score from several measurement
//! dimensions, each weighted according to [`ScoringWeights`].

/// Per-query measurement dimensions collected during a benchmark run.
#[derive(Debug, Clone, Default)]
pub struct QueryDimensions {
    /// Ra parse time in milliseconds.
    pub parse_ms: f64,
    /// Ra optimization time in milliseconds.
    pub optimize_ms: f64,
    /// Postgres `EXPLAIN` round-trip time in milliseconds (if available).
    pub pg_explain_ms: Option<f64>,
    /// Structural similarity score against Postgres plan \[0, 1\].
    pub structural_sim: Option<f64>,
    /// Ratio of Postgres total cost to Ra estimated cost.
    ///
    /// 1.0 means identical estimates; values far from 1.0 indicate
    /// divergence in cardinality or cost model assumptions.
    pub cost_ratio: Option<f64>,
    /// Ratio of Postgres actual rows to Ra estimated rows (from
    /// `EXPLAIN ANALYZE`).  Only present when `--execute` is used.
    pub actual_rows_ratio: Option<f64>,
}

/// Weights used to combine dimension scores into a single composite value.
///
/// Weights do not need to sum to 1.0; they are normalised internally.
#[derive(Debug, Clone)]
pub struct ScoringWeights {
    /// Weight for structural plan similarity.  Default: 0.25.
    pub structural: f64,
    /// Weight for cost-model accuracy.  Default: 0.30.
    pub cost_accuracy: f64,
    /// Weight for execution accuracy (actual vs. estimated rows).
    /// Default: 0.35.
    pub execution: f64,
    /// Weight for Ra speed relative to Postgres EXPLAIN.  Default: 0.10.
    pub speed: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            structural: 0.25,
            cost_accuracy: 0.30,
            execution: 0.35,
            speed: 0.10,
        }
    }
}

/// Compute a composite score in \[0, 1\] for a single query.
///
/// Dimensions without measurements are skipped and their weight is
/// redistributed proportionally to the dimensions that *do* have data.
/// Returns 0.0 if no dimension has data.
#[must_use]
pub fn compute_score(dims: &QueryDimensions, weights: &ScoringWeights) -> f64 {
    let mut total_weight = 0.0_f64;
    let mut weighted_sum = 0.0_f64;

    if let Some(sim) = dims.structural_sim {
        // Already in [0, 1].
        weighted_sum += weights.structural * sim.clamp(0.0, 1.0);
        total_weight += weights.structural;
    }

    if let Some(ratio) = dims.cost_ratio {
        // 1.0 when estimates are equal; decreases symmetrically for
        // over- and under-estimates.  Uses |ln(ratio)| so that 2× and
        // 0.5× divergences are penalised equally.
        let accuracy = cost_accuracy(ratio);
        weighted_sum += weights.cost_accuracy * accuracy;
        total_weight += weights.cost_accuracy;
    }

    if let Some(ratio) = dims.actual_rows_ratio {
        let accuracy = cost_accuracy(ratio);
        weighted_sum += weights.execution * accuracy;
        total_weight += weights.execution;
    }

    // Speed: fraction of Postgres EXPLAIN time used by Ra
    // (parse + optimize).  Capped at 1.0 (Ra faster than PG = full
    // score, not bonus).
    let ra_total_ms = dims.parse_ms + dims.optimize_ms;
    if let Some(pg_ms) = dims.pg_explain_ms {
        if ra_total_ms > 0.0 && pg_ms > 0.0 {
            let speed = (pg_ms / ra_total_ms).min(1.0);
            weighted_sum += weights.speed * speed;
            total_weight += weights.speed;
        }
    }

    if total_weight == 0.0 {
        return 0.0;
    }

    weighted_sum / total_weight
}

/// Convert a cost ratio into a [0, 1] accuracy score.
///
/// Uses `1 / (1 + |ln(ratio)|)` so that ratio = 1.0 → 1.0 and the
/// score decays symmetrically for factors above and below 1.
fn cost_accuracy(ratio: f64) -> f64 {
    if ratio <= 0.0 || !ratio.is_finite() {
        return 0.0;
    }
    1.0 / (1.0 + ratio.ln().abs())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_score_all_dimensions() {
        let dims = QueryDimensions {
            parse_ms: 1.0,
            optimize_ms: 1.0,
            pg_explain_ms: Some(10.0),
            structural_sim: Some(1.0),
            cost_ratio: Some(1.0),
            actual_rows_ratio: Some(1.0),
        };
        let score = compute_score(&dims, &ScoringWeights::default());
        assert!(
            (score - 1.0).abs() < 1e-6,
            "perfect inputs should score 1.0, got {score}"
        );
    }

    #[test]
    fn zero_score_no_data() {
        let dims = QueryDimensions {
            parse_ms: 1.0,
            optimize_ms: 1.0,
            ..QueryDimensions::default()
        };
        let score = compute_score(&dims, &ScoringWeights::default());
        assert_eq!(score, 0.0);
    }

    #[test]
    fn partial_score_only_structural() {
        let dims = QueryDimensions {
            structural_sim: Some(0.8),
            ..QueryDimensions::default()
        };
        let score = compute_score(&dims, &ScoringWeights::default());
        assert!(
            (score - 0.8).abs() < 1e-6,
            "single-dimension score should equal that dimension, got {score}"
        );
    }

    #[test]
    fn cost_accuracy_unity_is_one() {
        assert!((cost_accuracy(1.0) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn cost_accuracy_zero_ratio() {
        assert_eq!(cost_accuracy(0.0), 0.0);
    }

    #[test]
    fn cost_accuracy_symmetrical() {
        // 2× and 0.5× should yield identical accuracy
        let above = cost_accuracy(2.0);
        let below = cost_accuracy(0.5);
        assert!((above - below).abs() < 1e-10);
    }
}

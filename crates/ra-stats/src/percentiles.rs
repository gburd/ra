//! Percentile tracking with a t-digest algorithm.
//!
//! Provides efficient approximate quantile estimation over streaming
//! data. The t-digest merges incoming values into weighted centroids,
//! keeping more precision near the tails (p1, p99) while using fewer
//! centroids in the middle of the distribution.
//!
//! Queries for p50/p75/p90/p99 run in O(centroids) which is bounded
//! by the compression parameter (default 100), so effectively O(1).

use std::cmp::Ordering as CmpOrd;

/// A single centroid in the t-digest: a (mean, weight) pair.
#[derive(Debug, Clone, Copy)]
struct Centroid {
    mean: f64,
    weight: f64,
}

/// T-digest for streaming percentile estimation.
///
/// Maintains a compressed set of centroids that approximate the full
/// distribution. The `compression` parameter controls the accuracy
/// vs. memory trade-off (higher = more centroids = more accurate).
#[derive(Debug, Clone)]
pub struct TDigest {
    centroids: Vec<Centroid>,
    compression: f64,
    total_weight: f64,
    max_unmerged: usize,
    unmerged: Vec<Centroid>,
    min_val: f64,
    max_val: f64,
}

impl TDigest {
    /// Create a new t-digest with the given compression factor.
    ///
    /// Typical values: 100 (default) for ~1% accuracy at tails,
    /// 200 for higher accuracy. The number of centroids is bounded
    /// by roughly `compression * pi / 2`.
    pub fn new(compression: f64) -> Self {
        Self {
            centroids: Vec::new(),
            compression,
            total_weight: 0.0,
            max_unmerged: compression as usize,
            unmerged: Vec::new(),
            min_val: f64::INFINITY,
            max_val: f64::NEG_INFINITY,
        }
    }

    /// Add a single value to the digest.
    pub fn add(&mut self, value: f64) {
        self.add_weighted(value, 1.0);
    }

    /// Add a weighted value to the digest.
    pub fn add_weighted(&mut self, value: f64, weight: f64) {
        if weight <= 0.0 {
            return;
        }
        if value < self.min_val {
            self.min_val = value;
        }
        if value > self.max_val {
            self.max_val = value;
        }
        self.unmerged.push(Centroid {
            mean: value,
            weight,
        });
        self.total_weight += weight;

        if self.unmerged.len() >= self.max_unmerged {
            self.compress();
        }
    }

    /// Merge buffered values into the centroid list.
    fn compress(&mut self) {
        if self.unmerged.is_empty() {
            return;
        }

        let mut all: Vec<Centroid> =
            self.centroids.drain(..).chain(self.unmerged.drain(..)).collect();

        all.sort_by(|a, b| {
            a.mean.partial_cmp(&b.mean).unwrap_or(CmpOrd::Equal)
        });

        if all.is_empty() {
            return;
        }

        let total = self.total_weight;
        let mut result = Vec::with_capacity(all.len());
        let mut current = all[0];
        let mut weight_so_far = 0.0;

        for incoming in all.into_iter().skip(1) {
            let q = (weight_so_far + current.weight / 2.0) / total;
            let max_weight = Self::max_weight(q, self.compression, total);

            if current.weight + incoming.weight <= max_weight {
                let new_weight = current.weight + incoming.weight;
                current.mean = current.mean
                    + (incoming.mean - current.mean) * incoming.weight
                        / new_weight;
                current.weight = new_weight;
            } else {
                weight_so_far += current.weight;
                result.push(current);
                current = incoming;
            }
        }
        result.push(current);
        self.centroids = result;
    }

    /// Maximum centroid weight at quantile `q` given the compression
    /// factor. The formula `4 * n * q * (1 - q) / compression`
    /// allocates more resolution at the tails (q near 0 or 1).
    fn max_weight(q: f64, compression: f64, n: f64) -> f64 {
        4.0 * n * q * (1.0 - q) / compression
    }

    /// Estimate the value at quantile `q` (0.0 = min, 1.0 = max).
    pub fn quantile(&mut self, q: f64) -> Option<f64> {
        self.compress();

        if self.centroids.is_empty() {
            return None;
        }

        let q = q.clamp(0.0, 1.0);

        if (q - 0.0).abs() < f64::EPSILON {
            return Some(self.min_val);
        }
        if (q - 1.0).abs() < f64::EPSILON {
            return Some(self.max_val);
        }

        let target = q * self.total_weight;
        let mut cumulative = 0.0;

        let len = self.centroids.len();
        for (i, c) in self.centroids.iter().enumerate() {
            let half_w = c.weight / 2.0;

            if cumulative + half_w >= target {
                // Interpolate within the first half of this centroid.
                if i == 0 {
                    let t = target / half_w;
                    return Some(self.min_val + t * (c.mean - self.min_val));
                }
                let prev = &self.centroids[i - 1];
                let gap = c.mean - prev.mean;
                let prev_half = prev.weight / 2.0;
                let range = prev_half + half_w;
                let offset = target - (cumulative - prev_half);
                return Some(prev.mean + gap * offset / range);
            }

            cumulative += c.weight;

            if cumulative >= target {
                if i == len - 1 {
                    let overshoot = cumulative - target;
                    let t = overshoot / half_w;
                    return Some(
                        self.max_val - t * (self.max_val - c.mean),
                    );
                }
                let next = &self.centroids[i + 1];
                let gap = next.mean - c.mean;
                let next_half = next.weight / 2.0;
                let range = half_w + next_half;
                let offset = target - (cumulative - half_w);
                return Some(c.mean + gap * offset / range);
            }
        }

        Some(self.max_val)
    }

    /// Return the p50 (median).
    pub fn p50(&mut self) -> Option<f64> {
        self.quantile(0.50)
    }

    /// Return the p75.
    pub fn p75(&mut self) -> Option<f64> {
        self.quantile(0.75)
    }

    /// Return the p90.
    pub fn p90(&mut self) -> Option<f64> {
        self.quantile(0.90)
    }

    /// Return the p99.
    pub fn p99(&mut self) -> Option<f64> {
        self.quantile(0.99)
    }

    /// Total number of values added.
    pub fn count(&self) -> f64 {
        self.total_weight
    }

    /// Number of centroids in the digest.
    pub fn centroid_count(&self) -> usize {
        self.centroids.len() + self.unmerged.len()
    }

    /// Whether the digest has any data.
    pub fn is_empty(&self) -> bool {
        self.total_weight == 0.0
    }

    /// Minimum value seen.
    pub fn min(&self) -> Option<f64> {
        if self.is_empty() {
            None
        } else {
            Some(self.min_val)
        }
    }

    /// Maximum value seen.
    pub fn max(&self) -> Option<f64> {
        if self.is_empty() {
            None
        } else {
            Some(self.max_val)
        }
    }

    /// Reset the digest, discarding all data.
    pub fn clear(&mut self) {
        self.centroids.clear();
        self.unmerged.clear();
        self.total_weight = 0.0;
        self.min_val = f64::INFINITY;
        self.max_val = f64::NEG_INFINITY;
    }
}

impl Default for TDigest {
    fn default() -> Self {
        Self::new(100.0)
    }
}

/// Convenience container tracking named percentile metrics.
#[derive(Debug, Clone)]
pub struct PercentileTracker {
    digest: TDigest,
    name: String,
}

impl PercentileTracker {
    /// Create a tracker with a label and default compression.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            digest: TDigest::default(),
            name: name.into(),
        }
    }

    /// Create a tracker with custom compression factor.
    pub fn with_compression(
        name: impl Into<String>,
        compression: f64,
    ) -> Self {
        Self {
            digest: TDigest::new(compression),
            name: name.into(),
        }
    }

    /// Record a sample.
    pub fn record(&mut self, value: f64) {
        self.digest.add(value);
    }

    /// The tracker's label.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Current summary: (p50, p75, p90, p99, count).
    pub fn summary(&mut self) -> Option<PercentileSummary> {
        let p50 = self.digest.p50()?;
        let p75 = self.digest.p75()?;
        let p90 = self.digest.p90()?;
        let p99 = self.digest.p99()?;
        Some(PercentileSummary {
            p50,
            p75,
            p90,
            p99,
            count: self.digest.count(),
            min: self.digest.min_val,
            max: self.digest.max_val,
        })
    }

    /// Mutable access to the underlying digest.
    pub fn digest_mut(&mut self) -> &mut TDigest {
        &mut self.digest
    }

    /// Read access to the underlying digest.
    pub fn digest(&self) -> &TDigest {
        &self.digest
    }

    /// Reset the tracker.
    pub fn clear(&mut self) {
        self.digest.clear();
    }
}

/// Snapshot of percentile values.
#[derive(Debug, Clone, Copy)]
pub struct PercentileSummary {
    /// Median.
    pub p50: f64,
    /// 75th percentile.
    pub p75: f64,
    /// 90th percentile.
    pub p90: f64,
    /// 99th percentile.
    pub p99: f64,
    /// Total observations.
    pub count: f64,
    /// Observed minimum.
    pub min: f64,
    /// Observed maximum.
    pub max: f64,
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::expect_used,
    clippy::cast_lossless
)]
mod tests {
    use super::*;

    #[test]
    fn empty_digest_returns_none() {
        let mut td = TDigest::default();
        assert!(td.quantile(0.5).is_none());
        assert!(td.p50().is_none());
        assert!(td.is_empty());
    }

    #[test]
    fn single_value() {
        let mut td = TDigest::default();
        td.add(42.0);
        assert_eq!(td.count(), 1.0);
        let p50 = td.p50().expect("should have p50");
        assert!((p50 - 42.0).abs() < 1.0);
    }

    #[test]
    fn min_max_tracked() {
        let mut td = TDigest::default();
        td.add(10.0);
        td.add(50.0);
        td.add(1.0);
        assert_eq!(td.min(), Some(1.0));
        assert_eq!(td.max(), Some(50.0));
    }

    #[test]
    fn uniform_distribution_percentiles() {
        let mut td = TDigest::new(200.0);
        for i in 1..=1000 {
            td.add(i as f64);
        }
        let p50 = td.p50().expect("p50");
        let p90 = td.p90().expect("p90");
        let p99 = td.p99().expect("p99");

        // Loose bounds: t-digest is approximate
        assert!(
            (p50 - 500.0).abs() < 50.0,
            "p50 = {p50}, expected ~500"
        );
        assert!(
            (p90 - 900.0).abs() < 50.0,
            "p90 = {p90}, expected ~900"
        );
        assert!(
            (p99 - 990.0).abs() < 50.0,
            "p99 = {p99}, expected ~990"
        );
    }

    #[test]
    fn quantile_clamping() {
        let mut td = TDigest::default();
        td.add(1.0);
        td.add(100.0);
        let q0 = td.quantile(0.0).expect("q0");
        let q1 = td.quantile(1.0).expect("q1");
        assert!((q0 - 1.0).abs() < f64::EPSILON);
        assert!((q1 - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn quantile_out_of_range_clamped() {
        let mut td = TDigest::default();
        td.add(5.0);
        let below = td.quantile(-0.5).expect("below");
        let above = td.quantile(1.5).expect("above");
        assert!((below - 5.0).abs() < 1.0);
        assert!((above - 5.0).abs() < 1.0);
    }

    #[test]
    fn clear_resets() {
        let mut td = TDigest::default();
        td.add(1.0);
        td.add(2.0);
        td.clear();
        assert!(td.is_empty());
        assert!(td.p50().is_none());
    }

    #[test]
    fn zero_weight_ignored() {
        let mut td = TDigest::default();
        td.add_weighted(99.0, 0.0);
        assert!(td.is_empty());
    }

    #[test]
    fn tracker_summary() {
        let mut tracker = PercentileTracker::new("latency");
        assert_eq!(tracker.name(), "latency");
        for i in 1..=100 {
            tracker.record(i as f64);
        }
        let s = tracker.summary().expect("summary");
        assert!(s.p50 > 0.0);
        assert!(s.p90 > s.p50);
        assert!(s.p99 > s.p90);
        assert_eq!(s.count, 100.0);
        assert!((s.min - 1.0).abs() < f64::EPSILON);
        assert!((s.max - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tracker_clear() {
        let mut tracker = PercentileTracker::new("test");
        tracker.record(1.0);
        tracker.clear();
        assert!(tracker.summary().is_none());
    }

    #[test]
    fn compression_affects_centroid_count() {
        let mut low = TDigest::new(10.0);
        let mut high = TDigest::new(500.0);
        for i in 0..10_000 {
            low.add(i as f64);
            high.add(i as f64);
        }
        low.p50();
        high.p50();
        assert!(
            low.centroid_count() < high.centroid_count(),
            "low compression ({}) should have fewer centroids than high ({})",
            low.centroid_count(),
            high.centroid_count()
        );
    }

    #[test]
    fn large_dataset() {
        let mut td = TDigest::new(100.0);
        for i in 0..100_000 {
            td.add(i as f64);
        }
        let p50 = td.p50().expect("p50");
        assert!(
            (p50 - 50_000.0).abs() < 5_000.0,
            "p50 = {p50}, expected ~50000"
        );
    }
}

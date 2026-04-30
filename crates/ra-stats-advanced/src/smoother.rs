//! Exponentially Weighted Moving Average (EWMA) smoother.
//!
//! Filters noise from streaming statistics by applying exponential
//! decay to older observations. Newer samples have higher weight,
//! so the smoothed value tracks recent trends while dampening
//! transient spikes.

/// EWMA smoother for a single metric.
///
/// The smoothed value is updated on each call to [`update`](Self::update):
///
/// ```text
/// smoothed = alpha * new_value + (1 - alpha) * smoothed
/// ```
///
/// where `alpha` controls how quickly old data is forgotten.
/// - `alpha` close to 1.0: follows raw data closely (minimal smoothing)
/// - `alpha` close to 0.0: heavy smoothing, slow to react
#[derive(Debug, Clone)]
pub struct Ewma {
    alpha: f64,
    value: Option<f64>,
    count: u64,
}

impl Ewma {
    /// Create a smoother with the given decay factor `alpha` in `(0, 1]`.
    ///
    /// # Panics
    ///
    /// Panics if `alpha` is not in `(0.0, 1.0]`.
    pub fn new(alpha: f64) -> Self {
        assert!(
            alpha > 0.0 && alpha <= 1.0,
            "alpha must be in (0.0, 1.0], got {alpha}"
        );
        Self {
            alpha,
            value: None,
            count: 0,
        }
    }

    /// Create a smoother tuned for an N-sample half-life.
    ///
    /// After `half_life` samples, an old observation's weight decays
    /// to 50% of its original influence.
    ///
    /// # Panics
    ///
    /// Panics if `half_life` is zero.
    pub fn from_half_life(half_life: u64) -> Self {
        assert!(half_life > 0, "half_life must be > 0");
        let alpha = 1.0 - 0.5_f64.powf(1.0 / half_life as f64);
        Self::new(alpha)
    }

    /// Feed a new observation and return the smoothed value.
    pub fn update(&mut self, value: f64) -> f64 {
        self.count += 1;
        let smoothed = match self.value {
            Some(prev) => self.alpha * value + (1.0 - self.alpha) * prev,
            None => value,
        };
        self.value = Some(smoothed);
        smoothed
    }

    /// Current smoothed value, or `None` if no data has been fed.
    pub fn value(&self) -> Option<f64> {
        self.value
    }

    /// Number of observations fed so far.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The decay factor.
    pub fn alpha(&self) -> f64 {
        self.alpha
    }

    /// Reset the smoother, discarding state.
    pub fn reset(&mut self) {
        self.value = None;
        self.count = 0;
    }
}

/// A collection of named EWMA smoothers.
///
/// Useful when tracking several metrics (CPU, memory, I/O) with
/// the same smoothing parameters.
#[derive(Debug, Clone)]
pub struct SmootherSet {
    alpha: f64,
    smoothers: Vec<(String, Ewma)>,
}

impl SmootherSet {
    /// Create an empty set with the given alpha applied to all members.
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha,
            smoothers: Vec::new(),
        }
    }

    /// Add a named smoother.
    pub fn add(&mut self, name: impl Into<String>) {
        let name = name.into();
        if !self.smoothers.iter().any(|(n, _)| n == &name) {
            self.smoothers.push((name, Ewma::new(self.alpha)));
        }
    }

    /// Update a named smoother and return the smoothed value.
    ///
    /// Returns `None` if the name is not registered.
    pub fn update(&mut self, name: &str, value: f64) -> Option<f64> {
        for (n, smoother) in &mut self.smoothers {
            if n == name {
                return Some(smoother.update(value));
            }
        }
        None
    }

    /// Get the current smoothed value for a metric.
    pub fn get(&self, name: &str) -> Option<f64> {
        for (n, smoother) in &self.smoothers {
            if n == name {
                return smoother.value();
            }
        }
        None
    }

    /// Reset all smoothers.
    pub fn reset_all(&mut self) {
        for (_, smoother) in &mut self.smoothers {
            smoother.reset();
        }
    }

    /// Number of registered metrics.
    pub fn len(&self) -> usize {
        self.smoothers.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.smoothers.is_empty()
    }
}

#[expect(
    clippy::float_cmp,
    reason = "exact float equality needed for deterministic stats tests"
)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let ewma = Ewma::new(0.5);
        assert!(ewma.value().is_none());
        assert_eq!(ewma.count(), 0);
        assert_eq!(ewma.alpha(), 0.5);
    }

    #[test]
    #[should_panic(expected = "alpha must be in")]
    fn invalid_alpha_zero() {
        let _ = Ewma::new(0.0);
    }

    #[test]
    #[should_panic(expected = "alpha must be in")]
    fn invalid_alpha_negative() {
        let _ = Ewma::new(-0.1);
    }

    #[test]
    #[should_panic(expected = "alpha must be in")]
    fn invalid_alpha_too_large() {
        let _ = Ewma::new(1.1);
    }

    #[test]
    fn first_update_returns_raw_value() {
        let mut ewma = Ewma::new(0.5);
        let v = ewma.update(100.0);
        assert!((v - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn smoothing_dampens_spike() {
        let mut ewma = Ewma::new(0.1);
        // Establish baseline
        for _ in 0..20 {
            ewma.update(100.0);
        }
        let before = ewma.value().expect("should have value");
        // Introduce spike
        ewma.update(1000.0);
        let after = ewma.value().expect("should have value");
        // Smoothed value should be pulled toward 1000 but not reach it
        assert!(after > before);
        assert!(after < 1000.0);
    }

    #[test]
    fn alpha_one_no_smoothing() {
        let mut ewma = Ewma::new(1.0);
        ewma.update(10.0);
        ewma.update(20.0);
        let v = ewma.value().expect("val");
        assert!((v - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_clears_state() {
        let mut ewma = Ewma::new(0.5);
        ewma.update(42.0);
        ewma.reset();
        assert!(ewma.value().is_none());
        assert_eq!(ewma.count(), 0);
    }

    #[test]
    fn count_tracks_updates() {
        let mut ewma = Ewma::new(0.5);
        ewma.update(1.0);
        ewma.update(2.0);
        ewma.update(3.0);
        assert_eq!(ewma.count(), 3);
    }

    #[test]
    fn from_half_life() {
        let ewma = Ewma::from_half_life(10);
        assert!(ewma.alpha() > 0.0);
        assert!(ewma.alpha() < 1.0);
    }

    #[test]
    #[should_panic(expected = "half_life must be > 0")]
    fn from_half_life_zero_panics() {
        let _ = Ewma::from_half_life(0);
    }

    #[test]
    fn half_life_decay_property() {
        let mut ewma = Ewma::from_half_life(10);
        // Feed a single 1.0 then lots of 0.0
        ewma.update(1.0);
        for _ in 0..10 {
            ewma.update(0.0);
        }
        let val = ewma.value().expect("val");
        // After half_life steps of 0.0, the original should decay to ~0.5
        assert!((val - 0.5).abs() < 0.15, "expected ~0.5, got {val}");
    }

    // ---- SmootherSet ----

    #[test]
    fn set_add_and_update() {
        let mut set = SmootherSet::new(0.5);
        set.add("cpu");
        set.add("memory");
        assert_eq!(set.len(), 2);

        let cpu = set.update("cpu", 80.0).expect("cpu");
        assert!((cpu - 80.0).abs() < f64::EPSILON);

        let missing = set.update("disk", 10.0);
        assert!(missing.is_none());
    }

    #[test]
    fn set_get() {
        let mut set = SmootherSet::new(0.5);
        set.add("cpu");
        assert!(set.get("cpu").is_none());
        set.update("cpu", 50.0);
        assert!(set.get("cpu").is_some());
    }

    #[test]
    fn set_no_duplicates() {
        let mut set = SmootherSet::new(0.5);
        set.add("cpu");
        set.add("cpu");
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn set_reset_all() {
        let mut set = SmootherSet::new(0.5);
        set.add("a");
        set.add("b");
        set.update("a", 1.0);
        set.update("b", 2.0);
        set.reset_all();
        assert!(set.get("a").is_none());
        assert!(set.get("b").is_none());
    }

    #[test]
    fn set_is_empty() {
        let set = SmootherSet::new(0.5);
        assert!(set.is_empty());
    }
}

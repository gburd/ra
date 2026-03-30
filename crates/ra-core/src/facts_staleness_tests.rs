//! Tests for statistics staleness detection.

#[cfg(test)]
mod tests {
    use super::super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn mock_table_stats(
        row_count: f64,
        modifications: u64,
        days_old: i64,
    ) -> TableStats {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        TableStats {
            row_count,
            page_count: 1000,
            average_row_size: 100.0,
            table_size_bytes: 1_000_000,
            live_tuples: Some(row_count),
            dead_tuples: Some(0.0),
            last_analyzed: Some(now - (days_old * 86400)),
            confidence: 1.0,
            estimated_modifications: modifications,
        }
    }

    #[test]
    fn fresh_stats_low_modification_rate() {
        // < 1% modification rate
        let stats = mock_table_stats(100_000.0, 500, 0);
        let factor = stats.staleness_factor();
        assert!((1.0..1.5).contains(&factor), "Expected fresh stats, got {factor}");
        assert!(stats.is_fresh());
        assert!(!stats.is_stale());
    }

    #[test]
    fn slightly_stale_moderate_modifications() {
        // 2% modification rate
        let stats = mock_table_stats(100_000.0, 2_000, 1);
        let factor = stats.staleness_factor();
        assert!((1.2..2.0).contains(&factor), "Expected slightly stale, got {factor}");
    }

    #[test]
    fn moderately_stale_significant_modifications() {
        // 15% modification rate
        let stats = mock_table_stats(100_000.0, 15_000, 2);
        let factor = stats.staleness_factor();
        assert!((2.0..5.0).contains(&factor), "Expected moderately stale, got {factor}");
        assert!(stats.is_stale());
    }

    #[test]
    fn very_stale_high_modifications() {
        // 40% modification rate
        let stats = mock_table_stats(100_000.0, 40_000, 3);
        let factor = stats.staleness_factor();
        assert!((5.0..=10.0).contains(&factor), "Expected very stale, got {factor}");
        assert!(stats.is_stale());
    }

    #[test]
    fn extremely_stale_table_doubled() {
        // 100% modification rate (table doubled in size)
        let stats = mock_table_stats(100_000.0, 100_000, 5);
        let factor = stats.staleness_factor();
        assert!((factor - 10.0).abs() < 0.01, "Expected max penalty");
        assert!(stats.is_stale());
    }

    #[test]
    fn age_based_staleness() {
        // Very old stats, even with no modifications
        let stats = mock_table_stats(100_000.0, 0, 100);
        let factor = stats.staleness_factor();
        assert!(factor >= 3.0, "Expected age-based staleness, got {factor}");
        assert!(stats.is_stale());
    }

    #[test]
    fn combined_age_and_modifications() {
        // Both old and modified
        let stats = mock_table_stats(100_000.0, 30_000, 45);
        let factor = stats.staleness_factor();
        // Should use the maximum of both factors
        assert!(factor >= 5.0, "Expected combined staleness, got {factor}");
        assert!((factor - 10.0).abs() < 0.01, "Should be capped at max penalty");
    }

    #[test]
    fn no_analysis_time() {
        let mut stats = mock_table_stats(100_000.0, 1_000, 1);
        stats.last_analyzed = None;
        let factor = stats.staleness_factor();
        assert!(factor >= 5.0, "Expected high penalty for unknown analysis time");
        assert!(stats.is_stale());
    }

    #[test]
    fn empty_table() {
        let stats = mock_table_stats(0.0, 100, 1);
        let factor = stats.staleness_factor();
        // Should handle division by zero gracefully
        assert!((1.0..=10.0).contains(&factor));
    }

    #[test]
    fn staleness_capped_at_max() {
        // Extreme case: should still cap at 10.0
        let stats = mock_table_stats(1000.0, 10_000, 365);
        let factor = stats.staleness_factor();
        assert!((factor - 10.0).abs() < 0.01, "Staleness should be capped at 10.0");
    }

    #[test]
    fn recent_analysis_no_mods() {
        // Fresh stats: analyzed today, no modifications
        let stats = mock_table_stats(100_000.0, 0, 0);
        let factor = stats.staleness_factor();
        assert!((factor - 1.0).abs() < 0.01, "Expected no penalty for fresh stats");
        assert!(stats.is_fresh());
        assert!(!stats.is_stale());
    }

    #[test]
    fn one_week_old_no_mods() {
        // One week old but no modifications
        let stats = mock_table_stats(100_000.0, 0, 7);
        let factor = stats.staleness_factor();
        assert!((1.0..3.0).contains(&factor), "Expected minor age penalty, got {factor}");
        assert!(stats.is_fresh());
    }

    #[test]
    fn one_month_old() {
        let stats = mock_table_stats(100_000.0, 500, 30);
        let factor = stats.staleness_factor();
        assert!(factor >= 2.0, "Expected staleness for month-old stats");
    }

    #[test]
    fn three_months_old() {
        let stats = mock_table_stats(100_000.0, 1_000, 90);
        let factor = stats.staleness_factor();
        assert!(factor >= 3.0, "Expected high staleness for 3-month-old stats");
        assert!(stats.is_stale());
    }
}

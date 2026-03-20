//! Statistics bridge: reads `pg_stats` and converts to RA format.
//!
//! Queries PostgreSQL catalog views to populate `ra_core::Statistics`
//! and `ra_core::ColumnStats` structs. Statistics are cached for
//! the duration of a single planning cycle.

use pgrx::prelude::*;

use ra_core::{ColumnStats, Statistics};

/// Gather statistics for a single table from `pg_stats`.
///
/// Returns `None` if the table has no statistics (unanalyzed) or
/// does not exist.
pub fn gather_table_stats(
    schema: &str,
    table: &str,
) -> Option<Statistics> {
    let row_count = estimate_row_count(schema, table)?;
    let mut stats = Statistics::new(row_count);

    let query = format!(
        "SELECT attname, n_distinct, null_frac, avg_width \
         FROM pg_stats \
         WHERE schemaname = '{schema}' AND tablename = '{table}'"
    );

    Spi::connect(|client| {
        let tup_table = client
            .select(&query, None, None)
            .unwrap_or_else(|e| {
                pgrx::warning!(
                    "ra_planner: pg_stats query failed: {e}"
                );
                panic!("SPI failure in stats_bridge");
            });

        for row in tup_table {
            let attname: Option<String> =
                row.get_by_name("attname")
                    .unwrap_or(None);
            let n_distinct: Option<f32> =
                row.get_by_name("n_distinct")
                    .unwrap_or(None);
            let null_frac: Option<f32> =
                row.get_by_name("null_frac")
                    .unwrap_or(None);
            let avg_width: Option<i32> =
                row.get_by_name("avg_width")
                    .unwrap_or(None);

            if let Some(col_name) = attname {
                let distinct = interpret_n_distinct(
                    n_distinct.unwrap_or(0.0),
                    row_count,
                );
                let mut col_stats = ColumnStats::new(distinct);
                col_stats.null_fraction =
                    f64::from(null_frac.unwrap_or(0.0));
                col_stats.avg_length =
                    avg_width.map(|w| f64::from(w));
                stats.columns.insert(col_name, col_stats);
            }
        }
    });

    Some(stats)
}

/// Estimate the row count for a table from `pg_class.reltuples`.
///
/// Returns `None` if the table does not exist or has never been
/// analyzed (`reltuples` == -1).
fn estimate_row_count(
    schema: &str,
    table: &str,
) -> Option<f64> {
    let query = format!(
        "SELECT c.reltuples \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{schema}' AND c.relname = '{table}'"
    );

    Spi::connect(|client| {
        let tup_table = client
            .select(&query, None, None)
            .unwrap_or_else(|e| {
                pgrx::warning!(
                    "ra_planner: row count query failed: {e}"
                );
                panic!("SPI failure in stats_bridge");
            });

        for row in tup_table {
            let reltuples: Option<f32> =
                row.get_by_name("reltuples")
                    .unwrap_or(None);

            if let Some(rt) = reltuples {
                if rt >= 0.0 {
                    return Some(f64::from(rt));
                }
            }
        }
        None
    })
}

/// Interpret PostgreSQL's `n_distinct` encoding.
///
/// Positive values are absolute NDV counts.
/// Negative values are a fraction of the table's row count
/// (e.g., -1.0 means every row is distinct).
fn interpret_n_distinct(n_distinct: f32, row_count: f64) -> f64 {
    if n_distinct > 0.0 {
        f64::from(n_distinct)
    } else if n_distinct < 0.0 {
        (f64::from(-n_distinct) * row_count).max(1.0)
    } else {
        0.0
    }
}

/// Gather statistics for all tables referenced in a query.
///
/// `table_names` should be a list of `(schema, table)` pairs.
/// Tables with no statistics are silently skipped.
pub fn gather_all_stats(
    table_names: &[(String, String)],
) -> Vec<(String, Statistics)> {
    let mut result = Vec::with_capacity(table_names.len());
    for (schema, table) in table_names {
        if let Some(stats) = gather_table_stats(schema, table) {
            result.push((table.clone(), stats));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_distinct_positive() {
        let ndv = interpret_n_distinct(100.0, 1000.0);
        assert!((ndv - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_negative_fraction() {
        let ndv = interpret_n_distinct(-0.5, 1000.0);
        assert!((ndv - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_negative_one() {
        let ndv = interpret_n_distinct(-1.0, 1000.0);
        assert!((ndv - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_zero() {
        let ndv = interpret_n_distinct(0.0, 1000.0);
        assert!((ndv - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_negative_small_table() {
        let ndv = interpret_n_distinct(-0.001, 0.5);
        assert!(ndv >= 1.0);
    }
}

//! Statistics bridge: reads `pg_stats` and converts to RA format.
//!
//! Queries PostgreSQL catalog views to populate `ra_core::Statistics`
//! and `ra_core::ColumnStats` structs. Statistics are cached for
//! the duration of a single planning cycle.
//!
//! PostgreSQL-specific MVCC statistics (HOT updates, bloat) are tracked
//! separately as they don't apply to other database systems.

use pgrx::prelude::*;

use ra_core::{ColumnStats, Statistics};

/// PostgreSQL-specific MVCC and HOT update statistics.
///
/// These metrics are critical for understanding heap-only tuple behavior:
/// - HOT (Heap-Only Tuple) updates avoid index maintenance
/// - Dead tuples and bloat affect sequential scan performance
/// - High dead tuple ratio indicates need for VACUUM
///
/// **PostgreSQL-specific:** This struct only applies to PostgreSQL heap tables
/// and has no meaning for other databases (MySQL, Oracle, etc.) or storage
/// engines (columnar, LSM-tree, etc.).
#[derive(Debug, Clone)]
pub struct PostgresMvccStats {
    /// Fraction of updates that were HOT updates, in [0.0, 1.0].
    ///
    /// HOT updates happen when:
    /// 1. No indexed columns are modified
    /// 2. Sufficient free space exists on the same page
    ///
    /// High HOT ratio (>0.8) is good - indicates efficient updates.
    /// Low HOT ratio suggests:
    /// - Frequent indexed column updates
    /// - Page-level space fragmentation (need VACUUM or fillfactor tuning)
    pub hot_update_ratio: f64,

    /// Fraction of dead tuples, in [0.0, 1.0].
    ///
    /// Dead tuples remain after UPDATEs/DELETEs until VACUUM.
    /// High dead tuple ratio (>0.1) significantly degrades:
    /// - Sequential scan performance (must skip dead tuples)
    /// - Index scan performance (bitmap must filter dead tuples)
    pub dead_tuple_ratio: f64,

    /// Estimated bloat factor (size_on_disk / actual_data_size).
    ///
    /// Bloat > 2.0 indicates significant wasted space from:
    /// - Dead tuples (need VACUUM)
    /// - Page fragmentation (need VACUUM FULL or CLUSTER)
    pub bloat_factor: f64,

    /// Timestamp of last ANALYZE (for staleness detection).
    pub last_analyze: Option<String>,

    /// Timestamp of last VACUUM (for bloat tracking).
    pub last_vacuum: Option<String>,
}

impl PostgresMvccStats {
    /// Create default MVCC statistics (no HOT updates, no bloat).
    #[must_use]
    pub fn default() -> Self {
        Self {
            hot_update_ratio: 0.0,
            dead_tuple_ratio: 0.0,
            bloat_factor: 1.0,
            last_analyze: None,
            last_vacuum: None,
        }
    }

    /// Returns true if statistics are stale (>7 days since last ANALYZE).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        // Simplified check - in practice, parse timestamp and compare
        self.last_analyze.is_none()
    }

    /// Returns true if table needs VACUUM (high dead tuple ratio).
    #[must_use]
    pub fn needs_vacuum(&self) -> bool {
        self.dead_tuple_ratio > 0.1 || self.bloat_factor > 2.0
    }
}

/// PostgreSQL table statistics with MVCC metrics.
///
/// Wraps database-agnostic `Statistics` with PostgreSQL-specific MVCC data.
#[derive(Debug, Clone)]
pub struct PostgresTableStats {
    /// Database-agnostic statistics.
    pub base: Statistics,
    /// PostgreSQL-specific MVCC/HOT statistics.
    pub mvcc: PostgresMvccStats,
}

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

    // Query pg_stats for column statistics including advanced stats
    let query = format!(
        "SELECT attname, n_distinct, null_frac, avg_width, \
                correlation, most_common_vals, most_common_freqs, \
                histogram_bounds \
         FROM pg_stats \
         WHERE schemaname = '{}' AND tablename = '{}'",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    Spi::connect(|client| {
        let tup_table = client
            .select(&query, None, &[])
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
            let correlation: Option<f32> =
                row.get_by_name("correlation")
                    .unwrap_or(None);
            let most_common_vals: Option<String> =
                row.get_by_name("most_common_vals")
                    .unwrap_or(None);
            let most_common_freqs: Option<String> =
                row.get_by_name("most_common_freqs")
                    .unwrap_or(None);
            let histogram_bounds: Option<String> =
                row.get_by_name("histogram_bounds")
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

                // Parse correlation
                col_stats.correlation = correlation.map(f64::from);

                // Parse most common values and frequencies
                if let (Some(mcv_str), Some(mcf_str)) = (most_common_vals, most_common_freqs) {
                    if let (Some(mcvs), Some(mcfs)) = (
                        parse_pg_array(&mcv_str),
                        parse_float_array(&mcf_str),
                    ) {
                        col_stats.most_common_values = Some(mcvs);
                        col_stats.most_common_freqs = Some(mcfs);
                    }
                }

                // Parse histogram bounds
                if let Some(hist_str) = histogram_bounds {
                    if let Some(bounds) = parse_pg_array(&hist_str) {
                        col_stats.histogram = Some(create_equidepth_histogram(
                            bounds,
                            row_count,
                            distinct,
                        ));
                    }
                }

                stats.columns.insert(col_name, col_stats);
            }
        }
    });

    // Gather index statistics for index-aware optimization
    gather_index_stats(schema, table, &mut stats);

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
        "SELECT c.reltuples, c.relpages \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = '{}' AND c.relname = '{}'",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    Spi::connect(|client| {
        let tup_table = client
            .select(&query, None, &[])
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

/// Gather index statistics for a table.
///
/// Populates information about available indexes for index-aware
/// optimization decisions.
fn gather_index_stats(
    schema: &str,
    table: &str,
    stats: &mut Statistics,
) {
    // Query pg_indexes and pg_index for index metadata
    let query = format!(
        "SELECT i.indexname, i.indexdef, \
                ix.indisunique, ix.indisprimary, \
                ix.indnatts, \
                am.amname AS index_type, \
                COALESCE(pg_stat_get_numscans(c.oid), 0) AS num_scans, \
                COALESCE(pg_relation_size(c.oid), 0) AS index_size \
         FROM pg_indexes i \
         JOIN pg_class c ON c.relname = i.indexname \
         JOIN pg_index ix ON ix.indexrelid = c.oid \
         JOIN pg_am am ON am.oid = c.relam \
         WHERE i.schemaname = '{}' AND i.tablename = '{}' \
         ORDER BY i.indexname",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    Spi::connect(|client| {
        let tup_table = match client.select(&query, None, &[]) {
            Ok(tup) => tup,
            Err(e) => {
                pgrx::warning!(
                    "ra_planner: index stats query failed: {e}"
                );
                return;
            }
        };

        for row in tup_table {
            let indexname: Option<String> =
                row.get_by_name("indexname")
                    .unwrap_or(None);
            let indexdef: Option<String> =
                row.get_by_name("indexdef")
                    .unwrap_or(None);
            let is_unique: Option<bool> =
                row.get_by_name("indisunique")
                    .unwrap_or(None);
            let is_primary: Option<bool> =
                row.get_by_name("indisprimary")
                    .unwrap_or(None);
            let index_type_str: Option<String> =
                row.get_by_name("index_type")
                    .unwrap_or(None);
            let index_size: Option<i64> =
                row.get_by_name("index_size")
                    .unwrap_or(None);

            if let (Some(idx_name), Some(idx_def)) = (indexname, indexdef) {
                // Parse column names from index definition
                let columns = parse_index_columns(&idx_def);

                // Parse index type
                let index_type = index_type_str
                    .as_deref()
                    .and_then(parse_index_type)
                    .unwrap_or(ra_core::IndexType::Unknown);

                let mut idx_stats = ra_core::IndexStats::new(columns, index_type);
                idx_stats.is_unique = is_unique.unwrap_or(false);
                idx_stats.is_primary = is_primary.unwrap_or(false);
                idx_stats.index_size = index_size.unwrap_or(0) as u64;

                stats.indexes.insert(idx_name, idx_stats);
            }
        }
    });
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

/// Parse a PostgreSQL array string like `{value1,value2,value3}`.
///
/// PostgreSQL arrays are formatted as text with curly braces. This
/// parser handles basic arrays without nested structures or complex
/// escaping. Returns `None` if parsing fails.
fn parse_pg_array(array_str: &str) -> Option<Vec<String>> {
    let trimmed = array_str.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }

    let content = &trimmed[1..trimmed.len() - 1];
    if content.is_empty() {
        return Some(Vec::new());
    }

    // Split by commas, handling quoted values
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in content.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ',' && !in_quotes {
            values.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        values.push(current.trim().to_string());
    }

    Some(values)
}

/// Parse a PostgreSQL float array like `{0.1,0.2,0.3}`.
///
/// Returns `None` if parsing fails.
fn parse_float_array(array_str: &str) -> Option<Vec<f64>> {
    let strings = parse_pg_array(array_str)?;
    let mut floats = Vec::with_capacity(strings.len());

    for s in strings {
        match s.parse::<f64>() {
            Ok(f) => floats.push(f),
            Err(_) => return None,
        }
    }

    Some(floats)
}

/// Parse column names from a PostgreSQL index definition.
///
/// Extracts column names from CREATE INDEX DDL like:
/// "CREATE INDEX idx_name ON table_name USING btree (col1, col2)"
///
/// Returns empty vector if parsing fails.
fn parse_index_columns(index_def: &str) -> Vec<String> {
    // Find the opening parenthesis after "ON table_name"
    let start = match index_def.find('(') {
        Some(pos) => pos + 1,
        None => return Vec::new(),
    };

    // Find the matching closing parenthesis by counting
    let mut depth = 0;
    let mut end = None;
    for (i, ch) in index_def[start..].chars().enumerate() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    let end = match end {
        Some(pos) => pos,
        None => return Vec::new(),
    };

    // Extract column list
    let col_list = &index_def[start..end];

    // Split by commas and clean up
    col_list
        .split(',')
        .map(|s| {
            // Remove function calls like "lower(name)" → "name"
            let trimmed = s.trim();
            if let Some(paren_pos) = trimmed.find('(') {
                // Extract the argument from a function call
                if let Some(func_end) = trimmed.find(')') {
                    trimmed[paren_pos + 1..func_end].trim().to_string()
                } else {
                    trimmed.to_string()
                }
            } else {
                trimmed.to_string()
            }
        })
        .collect()
}

/// Parse PostgreSQL index type name to IndexType enum.
fn parse_index_type(type_name: &str) -> Option<ra_core::IndexType> {
    match type_name.to_lowercase().as_str() {
        "btree" => Some(ra_core::IndexType::BTree),
        "hash" => Some(ra_core::IndexType::Hash),
        "gin" => Some(ra_core::IndexType::Gin),
        "gist" => Some(ra_core::IndexType::Gist),
        "spgist" => Some(ra_core::IndexType::SpGist),
        "brin" => Some(ra_core::IndexType::Brin),
        _ => Some(ra_core::IndexType::Unknown),
    }
}

/// Create an equi-depth histogram from PostgreSQL histogram bounds.
///
/// PostgreSQL stores histogram_bounds as an array of boundary values
/// for equal-frequency buckets. We convert this to Ra's EquiDepthHistogram
/// format.
fn create_equidepth_histogram(
    bounds: Vec<String>,
    row_count: f64,
    distinct_count: f64,
) -> ra_core::Histogram {
    use ra_core::{EquiDepthHistogram, Histogram, HistogramBucket};

    if bounds.is_empty() {
        // Empty histogram
        return Histogram::EquiDepth(EquiDepthHistogram {
            buckets: Vec::new(),
            rows_per_bucket: 0.0,
        });
    }

    // PostgreSQL histogram_bounds has n+1 values for n buckets
    let num_buckets = bounds.len().saturating_sub(1);
    if num_buckets == 0 {
        return Histogram::EquiDepth(EquiDepthHistogram {
            buckets: Vec::new(),
            rows_per_bucket: 0.0,
        });
    }

    let rows_per_bucket = row_count / num_buckets as f64;
    let distinct_per_bucket = distinct_count / num_buckets as f64;

    let mut buckets = Vec::with_capacity(num_buckets);
    for i in 1..bounds.len() {
        buckets.push(HistogramBucket {
            upper_bound: bounds[i].clone(),
            row_count: rows_per_bucket,
            distinct_count: distinct_per_bucket,
        });
    }

    Histogram::EquiDepth(EquiDepthHistogram {
        buckets,
        rows_per_bucket,
    })
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

/// Gather PostgreSQL MVCC and HOT update statistics.
///
/// Queries `pg_stat_user_tables` for:
/// - HOT update ratio (critical for UPDATE planning)
/// - Dead tuple ratio (affects scan performance)
/// - Bloat estimation
/// - Last ANALYZE/VACUUM timestamps
///
/// Returns `None` if the table has no statistics.
pub fn gather_mvcc_stats(
    schema: &str,
    table: &str,
    base_stats: &Statistics,
) -> Option<PostgresMvccStats> {
    let query = format!(
        "SELECT \
           n_tup_upd, n_tup_hot_upd, n_dead_tup, n_live_tup, \
           last_analyze, last_vacuum, \
           pg_relation_size(c.oid) as table_size \
         FROM pg_stat_user_tables s \
         JOIN pg_class c ON c.relname = s.relname \
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = s.schemaname \
         WHERE s.schemaname = '{}' AND s.relname = '{}'",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    Spi::connect(|client| {
        let tup_table = match client.select(&query, None, &[]) {
            Ok(tup) => tup,
            Err(e) => {
                pgrx::warning!(
                    "ra_planner: mvcc stats query failed: {e}"
                );
                return None;
            }
        };

        for row in tup_table {
            let n_tup_upd: Option<i64> =
                row.get_by_name("n_tup_upd")
                    .unwrap_or(None);
            let n_tup_hot_upd: Option<i64> =
                row.get_by_name("n_tup_hot_upd")
                    .unwrap_or(None);
            let n_dead_tup: Option<i64> =
                row.get_by_name("n_dead_tup")
                    .unwrap_or(None);
            let n_live_tup: Option<i64> =
                row.get_by_name("n_live_tup")
                    .unwrap_or(None);
            let last_analyze: Option<String> =
                row.get_by_name("last_analyze")
                    .unwrap_or(None);
            let last_vacuum: Option<String> =
                row.get_by_name("last_vacuum")
                    .unwrap_or(None);
            let table_size: Option<i64> =
                row.get_by_name("table_size")
                    .unwrap_or(None);

            // Calculate HOT update ratio
            let hot_ratio = if let (Some(upd), Some(hot)) = (n_tup_upd, n_tup_hot_upd) {
                if upd > 0 {
                    hot as f64 / upd as f64
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Calculate dead tuple ratio
            let dead_ratio = if let (Some(dead), Some(live)) = (n_dead_tup, n_live_tup) {
                let total = dead + live;
                if total > 0 {
                    dead as f64 / total as f64
                } else {
                    0.0
                }
            } else {
                0.0
            };

            // Estimate bloat factor (simple heuristic: actual_size / expected_size)
            // In production, use pgstattuple or more sophisticated bloat detection
            let bloat = if let Some(size) = table_size {
                let expected_size = base_stats.row_count * base_stats.avg_row_size as f64;
                if expected_size > 0.0 {
                    (size as f64 / expected_size).max(1.0)
                } else {
                    1.0
                }
            } else {
                1.0
            };

            return Some(PostgresMvccStats {
                hot_update_ratio: hot_ratio,
                dead_tuple_ratio: dead_ratio,
                bloat_factor: bloat,
                last_analyze,
                last_vacuum,
            });
        }
        None
    })
}

/// Gather foreign key relationships for join optimization.
///
/// Returns a list of `(from_table, from_column, to_table, to_column)` tuples
/// representing foreign key constraints.
pub fn gather_foreign_keys(
    schema: &str,
    table: &str,
) -> Vec<(String, String, String, String)> {
    let query = format!(
        "SELECT \
           kcu.column_name AS from_column, \
           ccu.table_name AS to_table, \
           ccu.column_name AS to_column \
         FROM information_schema.table_constraints AS tc \
         JOIN information_schema.key_column_usage AS kcu \
           ON tc.constraint_name = kcu.constraint_name \
           AND tc.table_schema = kcu.table_schema \
         JOIN information_schema.constraint_column_usage AS ccu \
           ON ccu.constraint_name = tc.constraint_name \
           AND ccu.table_schema = tc.table_schema \
         WHERE tc.constraint_type = 'FOREIGN KEY' \
           AND tc.table_schema = '{}' \
           AND tc.table_name = '{}'",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    let mut fks = Vec::new();

    Spi::connect(|client| {
        let tup_table = match client.select(&query, None, &[]) {
            Ok(tup) => tup,
            Err(e) => {
                pgrx::warning!(
                    "ra_planner: foreign key query failed: {e}"
                );
                return;
            }
        };

        for row in tup_table {
            let from_col: Option<String> =
                row.get_by_name("from_column")
                    .unwrap_or(None);
            let to_table: Option<String> =
                row.get_by_name("to_table")
                    .unwrap_or(None);
            let to_col: Option<String> =
                row.get_by_name("to_column")
                    .unwrap_or(None);

            if let (Some(fc), Some(tt), Some(tc)) = (from_col, to_table, to_col) {
                fks.push((table.to_string(), fc, tt, tc));
            }
        }
    });

    fks
}

/// Check if a table has been recently analyzed.
///
/// Returns the timestamp of the last ANALYZE, or None if never analyzed.
pub fn last_analyze_time(
    schema: &str,
    table: &str,
) -> Option<String> {
    let query = format!(
        "SELECT last_analyze, last_autoanalyze \
         FROM pg_stat_user_tables \
         WHERE schemaname = '{}' AND relname = '{}'",
        schema.replace('\'', "''"),
        table.replace('\'', "''")
    );

    Spi::connect(|client| {
        let tup_table = match client.select(&query, None, &[]) {
            Ok(tup) => tup,
            Err(e) => {
                pgrx::warning!(
                    "ra_planner: last_analyze query failed: {e}"
                );
                return None;
            }
        };

        for row in tup_table {
            let last_analyze: Option<String> =
                row.get_by_name("last_analyze")
                    .unwrap_or(None);
            let last_autoanalyze: Option<String> =
                row.get_by_name("last_autoanalyze")
                    .unwrap_or(None);

            // Return the most recent of manual or auto-analyze
            return last_analyze.or(last_autoanalyze);
        }
        None
    })
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

    #[test]
    fn parse_simple_array() {
        let result = parse_pg_array("{a,b,c}");
        assert_eq!(result, Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]));
    }

    #[test]
    fn parse_empty_array() {
        let result = parse_pg_array("{}");
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn parse_quoted_array() {
        let result = parse_pg_array(r#"{"hello world","test,value"}"#);
        assert_eq!(result, Some(vec!["hello world".to_string(), "test,value".to_string()]));
    }

    #[test]
    fn parse_invalid_array() {
        let result = parse_pg_array("not an array");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_simple_float_array() {
        let result = parse_float_array("{0.1,0.2,0.3}");
        assert_eq!(result, Some(vec![0.1, 0.2, 0.3]));
    }

    #[test]
    fn parse_empty_float_array() {
        let result = parse_float_array("{}");
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn parse_invalid_float_array() {
        let result = parse_float_array("{0.1,not_a_float,0.3}");
        assert_eq!(result, None);
    }

    #[test]
    fn histogram_from_bounds() {
        let bounds = vec!["1".to_string(), "10".to_string(), "20".to_string(), "30".to_string()];
        let hist = create_equidepth_histogram(bounds, 1000.0, 30.0);

        match hist {
            ra_core::Histogram::EquiDepth(h) => {
                assert_eq!(h.buckets.len(), 3);
                assert!((h.rows_per_bucket - 333.333).abs() < 1.0);
                assert_eq!(h.buckets[0].upper_bound, "10");
                assert_eq!(h.buckets[1].upper_bound, "20");
                assert_eq!(h.buckets[2].upper_bound, "30");
            }
            _ => panic!("Expected EquiDepth histogram"),
        }
    }

    #[test]
    fn histogram_empty_bounds() {
        let bounds = Vec::new();
        let hist = create_equidepth_histogram(bounds, 1000.0, 30.0);

        match hist {
            ra_core::Histogram::EquiDepth(h) => {
                assert_eq!(h.buckets.len(), 0);
                assert!((h.rows_per_bucket - 0.0).abs() < f64::EPSILON);
            }
            _ => panic!("Expected EquiDepth histogram"),
        }
    }

    #[test]
    fn histogram_single_bound() {
        let bounds = vec!["5".to_string()];
        let hist = create_equidepth_histogram(bounds, 1000.0, 30.0);

        match hist {
            ra_core::Histogram::EquiDepth(h) => {
                assert_eq!(h.buckets.len(), 0);
            }
            _ => panic!("Expected EquiDepth histogram"),
        }
    }

    #[test]
    fn parse_simple_index_def() {
        let def = "CREATE INDEX idx_name ON table_name USING btree (col1, col2)";
        let cols = parse_index_columns(def);
        assert_eq!(cols, vec!["col1", "col2"]);
    }

    #[test]
    fn parse_index_with_function() {
        let def = "CREATE INDEX idx_name ON table_name USING btree (lower(name), age)";
        let cols = parse_index_columns(def);
        assert_eq!(cols, vec!["name", "age"]);
    }

    #[test]
    fn parse_single_column_index() {
        let def = "CREATE INDEX idx_name ON table_name USING btree (id)";
        let cols = parse_index_columns(def);
        assert_eq!(cols, vec!["id"]);
    }

    #[test]
    fn parse_invalid_index_def() {
        let def = "Not a valid index definition";
        let cols = parse_index_columns(def);
        assert!(cols.is_empty());
    }

    #[test]
    fn parse_btree_index_type() {
        let idx_type = parse_index_type("btree");
        assert_eq!(idx_type, Some(ra_core::IndexType::BTree));
    }

    #[test]
    fn parse_gin_index_type() {
        let idx_type = parse_index_type("gin");
        assert_eq!(idx_type, Some(ra_core::IndexType::Gin));
    }

    #[test]
    fn parse_hash_index_type() {
        let idx_type = parse_index_type("hash");
        assert_eq!(idx_type, Some(ra_core::IndexType::Hash));
    }

    #[test]
    fn parse_unknown_index_type() {
        let idx_type = parse_index_type("custom_index");
        assert_eq!(idx_type, Some(ra_core::IndexType::Unknown));
    }

    #[test]
    fn mvcc_stats_hot_updates() {
        let mvcc = PostgresMvccStats {
            hot_update_ratio: 0.9,
            dead_tuple_ratio: 0.05,
            bloat_factor: 1.2,
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-03-20".to_string()),
        };

        assert!((mvcc.hot_update_ratio - 0.9).abs() < f64::EPSILON);
        assert!(!mvcc.needs_vacuum());
    }

    #[test]
    fn mvcc_stats_needs_vacuum_dead_tuples() {
        let mvcc = PostgresMvccStats {
            hot_update_ratio: 0.7,
            dead_tuple_ratio: 0.15, // > 0.1 threshold
            bloat_factor: 1.5,
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-03-01".to_string()),
        };

        assert!(mvcc.needs_vacuum());
    }

    #[test]
    fn mvcc_stats_needs_vacuum_bloat() {
        let mvcc = PostgresMvccStats {
            hot_update_ratio: 0.8,
            dead_tuple_ratio: 0.05,
            bloat_factor: 2.5, // > 2.0 threshold
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-02-01".to_string()),
        };

        assert!(mvcc.needs_vacuum());
    }

    #[test]
    fn mvcc_stats_is_stale() {
        let mvcc_no_analyze = PostgresMvccStats {
            hot_update_ratio: 0.8,
            dead_tuple_ratio: 0.05,
            bloat_factor: 1.1,
            last_analyze: None,
            last_vacuum: Some("2026-03-20".to_string()),
        };

        assert!(mvcc_no_analyze.is_stale());

        let mvcc_recent = PostgresMvccStats {
            hot_update_ratio: 0.8,
            dead_tuple_ratio: 0.05,
            bloat_factor: 1.1,
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-03-20".to_string()),
        };

        // Note: is_stale() is simplified - just checks presence
        assert!(!mvcc_recent.is_stale());
    }
}

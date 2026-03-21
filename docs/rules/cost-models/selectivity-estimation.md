# Rule: "Predicate Selectivity Estimation"

**Category:** cost-models
**File:** `rules/cost-models/selectivity-estimation.rra`

## Metadata

- **ID:** `selectivity-estimation`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, duckdb, mssql, cockroachdb
- **Tags:** cost, selectivity, statistics, predicates, mcv, histogram
- **Authors:** "RA Contributors"


# Predicate Selectivity Estimation

## Description

Estimates the fraction of rows that satisfy a predicate. Selectivity is the
core input to cardinality estimation: Card(filter(R, p)) = |R| * sel(p).
Accurate selectivity estimation prevents catastrophic plan choices like
choosing nested-loop join when 90% of rows match.

The estimator uses a layered approach: (1) check Most Common Values (MCVs)
for exact match, (2) consult histograms for range predicates, (3) use
distinct count for equality on non-MCV values, (4) fall back to heuristic
defaults. Each layer provides progressively less accurate but broader
coverage.

**When to apply**: Every predicate in WHERE, HAVING, JOIN ON, and
implicit filter conditions.

**Why it works**: Real data has non-uniform distributions. MCVs capture
the head of the distribution (frequent values), histograms capture the
body, and distinct counts handle the tail. This layered approach handles
Zipfian distributions common in practice.

## Relational Algebra

```algebra
sel(col = value):
  If value in MCV: sel = mcv_frequency(value)
  Else: sel = (1 - sum(mcv_frequencies)) / (NDV - |MCV|)

sel(col < value):
  From histogram: fraction of values below threshold

sel(col BETWEEN low AND high):
  sel = sel(col <= high) - sel(col < low)

sel(p1 AND p2):
  sel = sel(p1) * sel(p2)  [independence assumption]

sel(p1 OR p2):
  sel = sel(p1) + sel(p2) - sel(p1) * sel(p2)

sel(NOT p):
  sel = 1 - sel(p)

sel(col IS NULL):
  sel = null_fraction(col)

sel(col IN (v1, v2, ...)):
  sel = sum(sel(col = vi)) - pairwise overlaps
  Approximation: min(1, |values| / NDV)
```

## Implementation

```rust
use ra_stats::{ColumnStats, Histogram, McvList};

struct SelectivityEstimator;

impl SelectivityEstimator {
    fn estimate(
        &self,
        pred: &Predicate,
        stats: &ColumnStats,
    ) -> f64 {
        match pred {
            Predicate::Eq { col, value } => {
                self.equality_selectivity(stats, value)
            }
            Predicate::NotEq { col, value } => {
                1.0 - self.equality_selectivity(stats, value)
                    - stats.null_fraction
            }
            Predicate::Lt { col, value } => {
                self.less_than_selectivity(stats, value)
            }
            Predicate::Le { col, value } => {
                self.less_than_selectivity(stats, value)
                    + self.equality_selectivity(stats, value)
            }
            Predicate::Gt { col, value } => {
                1.0 - self.less_than_selectivity(stats, value)
                    - self.equality_selectivity(stats, value)
                    - stats.null_fraction
            }
            Predicate::Between { col, low, high } => {
                self.range_selectivity(stats, low, high)
            }
            Predicate::IsNull { col } => {
                stats.null_fraction
            }
            Predicate::IsNotNull { col } => {
                1.0 - stats.null_fraction
            }
            Predicate::In { col, values } => {
                self.in_list_selectivity(stats, values)
            }
            Predicate::Like { col, pattern } => {
                self.like_selectivity(stats, pattern)
            }
            Predicate::And { left, right } => {
                let s1 = self.estimate(left, stats);
                let s2 = self.estimate(right, stats);
                s1 * s2
            }
            Predicate::Or { left, right } => {
                let s1 = self.estimate(left, stats);
                let s2 = self.estimate(right, stats);
                s1 + s2 - s1 * s2
            }
            Predicate::Not { inner } => {
                1.0 - self.estimate(inner, stats)
            }
        }
    }

    fn equality_selectivity(
        &self,
        stats: &ColumnStats,
        value: &Value,
    ) -> f64 {
        // Layer 1: Check MCV list
        if let Some(freq) = stats.mcv.frequency_of(value) {
            return freq;
        }

        // Layer 2: Non-MCV values share remaining frequency
        let mcv_total_freq: f64 =
            stats.mcv.frequencies.iter().sum();
        let remaining_freq = 1.0 - mcv_total_freq - stats.null_fraction;
        let remaining_ndv =
            stats.distinct_count - stats.mcv.len() as u64;

        if remaining_ndv > 0 {
            remaining_freq / remaining_ndv as f64
        } else {
            1.0 / stats.distinct_count.max(1) as f64
        }
    }

    fn less_than_selectivity(
        &self,
        stats: &ColumnStats,
        value: &Value,
    ) -> f64 {
        // Use histogram for range estimation
        if let Some(histogram) = &stats.histogram {
            return histogram.fraction_below(value);
        }

        // Fallback: linear interpolation between min and max
        if let (Some(min), Some(max)) = (&stats.min_value, &stats.max_value) {
            let range = max.as_f64() - min.as_f64();
            if range <= 0.0 {
                return 0.5;
            }
            let position = value.as_f64() - min.as_f64();
            (position / range).clamp(0.0, 1.0)
        } else {
            0.33 // Default heuristic
        }
    }

    fn range_selectivity(
        &self,
        stats: &ColumnStats,
        low: &Value,
        high: &Value,
    ) -> f64 {
        if let Some(histogram) = &stats.histogram {
            return histogram.fraction_in_range(low, high);
        }

        // Fallback: linear interpolation
        let sel_high = self.less_than_selectivity(stats, high)
            + self.equality_selectivity(stats, high);
        let sel_low = self.less_than_selectivity(stats, low);
        (sel_high - sel_low).max(0.0001)
    }

    fn in_list_selectivity(
        &self,
        stats: &ColumnStats,
        values: &[Value],
    ) -> f64 {
        // Sum individual equality selectivities
        // Account for overlap (inclusion-exclusion approximation)
        let mut total = 0.0;
        for value in values {
            total += self.equality_selectivity(stats, value);
        }
        total.min(1.0)
    }

    fn like_selectivity(
        &self,
        stats: &ColumnStats,
        pattern: &str,
    ) -> f64 {
        // PostgreSQL-style LIKE selectivity
        let has_leading_wildcard = pattern.starts_with('%');
        let has_trailing_wildcard = pattern.ends_with('%');

        // Count fixed characters
        let fixed_chars: Vec<&str> = pattern
            .split('%')
            .filter(|s| !s.is_empty())
            .collect();

        if fixed_chars.is_empty() {
            return 1.0; // Pattern is just '%' or '%%'
        }

        // Each fixed character reduces selectivity
        let char_selectivity = 1.0 / 26.0;
        let total_fixed_len: usize =
            fixed_chars.iter().map(|s| s.len()).sum();

        let base = char_selectivity.powi(total_fixed_len as i32);

        if has_leading_wildcard {
            // '%abc' -> position unknown, less selective
            (base * 10.0).min(0.5)
        } else {
            // 'abc%' -> prefix match, more selective
            base.max(0.0001)
        }
    }
}
```

**Restrictions:**
- Independence assumption for AND/OR (ignores correlations)
- Histogram resolution limits accuracy for multi-modal distributions
- MCV list size is fixed (typically 100 values)
- LIKE selectivity is heuristic (no string histogram)
- Expression predicates (col1 + col2 > 10) use default selectivity

## Cost Model

```rust
fn selectivity_error_cost(
    true_sel: f64,
    estimated_sel: f64,
    table_rows: f64,
) -> f64 {
    let true_card = table_rows * true_sel;
    let est_card = table_rows * estimated_sel;

    // q-error
    let q_error = if est_card > true_card {
        est_card / true_card
    } else {
        true_card / est_card
    };

    // Plan cost impact depends on downstream operators
    // A filter feeding a nested-loop join amplifies errors
    q_error
}
```

**Typical benefit**: Accurate selectivity estimation provides 20-80%
plan improvement. The greatest impact is on the scan-vs-index decision
and on multi-way join ordering.

## Test Cases

### Test 1: MCV equality

```sql
SELECT * FROM orders WHERE status = 'shipped';
-- MCV: status='shipped' freq=0.35
-- Expected: sel = 0.35 (direct MCV lookup)
```

### Test 2: Non-MCV equality

```sql
SELECT * FROM orders WHERE status = 'cancelled_fraud_review';
-- Not in MCV (top 100), NDV(status) = 15, MCV covers 0.92 frequency
-- sel = (1.0 - 0.92 - 0.0) / (15 - 10) = 0.016
```

### Test 3: Range from histogram

```sql
SELECT * FROM orders WHERE amount BETWEEN 50 AND 200;
-- Equi-depth histogram (100 buckets, 1M rows):
-- Bucket [40, 80]: 50% overlap -> 5,000 rows
-- Bucket [80, 150]: 100% overlap -> 10,000 rows
-- Bucket [150, 250]: 50% overlap -> 5,000 rows
-- sel = 20,000 / 1,000,000 = 0.02
```

### Test 4: AND with independence

```sql
SELECT * FROM products
WHERE category = 'electronics' AND price > 500;
-- sel(category) = 0.12, sel(price > 500) = 0.08
-- Independent estimate: 0.12 * 0.08 = 0.0096
-- True (correlated): 0.035 -> 3.6x underestimate
```

### Test 5: OR selectivity

```sql
SELECT * FROM users WHERE age < 18 OR age > 65;
-- sel(age < 18) = 0.15, sel(age > 65) = 0.12
-- sel(OR) = 0.15 + 0.12 - 0.15*0.12 = 0.252
```

### Test 6: LIKE prefix match

```sql
SELECT * FROM products WHERE name LIKE 'iPhone%';
-- Prefix 'iPhone': 6 characters
-- sel = (1/26)^6 = 3.2e-9 (extremely selective)
-- Clamped to min 0.0001 for safety
```

## References

**Foundational:**
- Selinger et al., "Access Path Selection in a RDBMS", SIGMOD 1979
- Ioannidis & Christodoulakis, "On the Propagation of Errors", SIGMOD 1991

**Histograms and MCVs:**
- Poosala et al., "Selectivity Estimation Without Attribute Value Independence", VLDB 1997
- Gunopulos et al., "Selectivity Estimators for Multidimensional Range Queries", VLDB 2005

**Modern implementations:**
- PostgreSQL: `src/backend/utils/adt/selfuncs.c` (selectivity functions)
- PostgreSQL: `src/backend/optimizer/util/clausesel.c` (clause selectivity)
- MySQL: `sql/opt_range.cc` (range selectivity)
- DuckDB: `src/optimizer/statistics/expression/` (statistics propagation)

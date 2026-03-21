# Rule: "System R Selectivity Estimation Formulas"

**Category:** cost-models
**File:** `rules/cost-models/system-r-selectivity-formulas.rra`

## Metadata

- **ID:** `system-r-selectivity-formulas`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, cockroachdb, mssql, oracle
- **Tags:** selectivity, estimation, system-r, statistics, cost-model, classic
- **Authors:** "Selinger, Astrahan, Chamberlin, Lorie, Price - IBM Research"


# System R Selectivity Estimation Formulas

## Description

The original selectivity estimation formulas from the System R optimizer. These
formulas estimate the fraction of tuples satisfying a predicate using only basic
statistics: the number of distinct values per column (ICARD) and the number of
tuples in the relation (TCARD). Despite their simplicity, these formulas are
still the foundation of selectivity estimation in every modern database.

System R assumed uniform data distribution and predicate independence. Later
systems added histograms and correlation tracking, but the base formulas remain
the default when detailed statistics are unavailable.

**When to apply**: Cost-based optimization requires selectivity estimates for
every predicate. These formulas provide the baseline estimates used when more
detailed statistics (histograms, MCVs) are not available.

**Why it works**: Under the uniformity assumption, equality predicates select
1/ICARD of the tuples (where ICARD is the number of distinct values). Range
predicates select a fraction proportional to the range width. These estimates
are often within an order of magnitude of the true selectivity, which is
sufficient for choosing between grossly different access paths.

## Relational Algebra

```algebra
Given relation R with TCARD(R) tuples and column A with ICARD(R.A) distinct values:

Selectivity formulas from Selinger et al. 1979:

1. col = value           -> F = 1 / ICARD(R.A)
2. col1 = col2           -> F = 1 / max(ICARD(R.A), ICARD(R.B))
3. col > value           -> F = (high_key - value) / (high_key - low_key)
                            (linear interpolation, default 1/3 if unknown)
4. col BETWEEN v1 AND v2 -> F = (v2 - v1) / (high_key - low_key)
5. col IN (v1, ..., vn)  -> F = min(n / ICARD(R.A), 0.5)
6. P1 AND P2             -> F = F(P1) * F(P2)  (independence assumption)
7. P1 OR P2              -> F = F(P1) + F(P2) - F(P1) * F(P2)
8. NOT P                 -> F = 1 - F(P)
9. col LIKE 'abc%'       -> F = 1/ICARD(R.A) if prefix is selective, else 1/10
10. col IS NULL           -> F = null_frac(R.A) if known, else 1/TCARD
```

## Implementation

```rust
use egg::{rewrite as rw, *};

struct SystemRSelectivityEstimator;

impl SystemRSelectivityEstimator {
    fn estimate(&self, pred: &Predicate, stats: &TableStats) -> f64 {
        match pred {
            Predicate::Eq { col, value: Value::Const(_) } => {
                // col = constant
                let icard = stats.distinct_count(col);
                if icard > 0 { 1.0 / icard as f64 } else { 1.0 / 10.0 }
            }

            Predicate::Eq { col: col1, value: Value::Col(col2) } => {
                // col1 = col2 (join predicate)
                let icard1 = stats.distinct_count(col1);
                let icard2 = stats.distinct_count(col2);
                let max_icard = icard1.max(icard2);
                if max_icard > 0 {
                    1.0 / max_icard as f64
                } else {
                    1.0 / 10.0
                }
            }

            Predicate::Range { col, low, high } => {
                // col BETWEEN low AND high
                let col_stats = stats.column_stats(col);
                if let (Some(lo), Some(hi)) = (col_stats.low_key, col_stats.high_key) {
                    let range = hi - lo;
                    if range > 0.0 {
                        (high - low) / range
                    } else {
                        1.0 / 3.0 // Default
                    }
                } else {
                    1.0 / 3.0 // Default when bounds unknown
                }
            }

            Predicate::Gt { col, value } => {
                let col_stats = stats.column_stats(col);
                if let (Some(hi), Some(lo)) = (col_stats.high_key, col_stats.low_key) {
                    let range = hi - lo;
                    if range > 0.0 {
                        (hi - *value) / range
                    } else {
                        1.0 / 3.0
                    }
                } else {
                    1.0 / 3.0 // System R default for unknown ranges
                }
            }

            Predicate::In { col, values } => {
                let icard = stats.distinct_count(col) as f64;
                let n = values.len() as f64;
                (n / icard).min(0.5)
            }

            Predicate::And(p1, p2) => {
                // Independence assumption
                self.estimate(p1, stats) * self.estimate(p2, stats)
            }

            Predicate::Or(p1, p2) => {
                let f1 = self.estimate(p1, stats);
                let f2 = self.estimate(p2, stats);
                f1 + f2 - f1 * f2
            }

            Predicate::Not(p) => {
                1.0 - self.estimate(p, stats)
            }

            Predicate::IsNull { col } => {
                stats.null_fraction(col)
                    .unwrap_or(1.0 / stats.table_cardinality() as f64)
            }

            _ => 1.0 / 10.0, // System R default for unknown predicates
        }
    }

    fn estimate_join_selectivity(
        &self,
        left_stats: &TableStats,
        right_stats: &TableStats,
        join_col_left: &Column,
        join_col_right: &Column,
    ) -> f64 {
        let icard_left = left_stats.distinct_count(join_col_left);
        let icard_right = right_stats.distinct_count(join_col_right);
        let max_icard = icard_left.max(icard_right);
        if max_icard > 0 {
            1.0 / max_icard as f64
        } else {
            1.0 / 10.0
        }
    }
}
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Always applicable as the baseline estimator
    // More accurate with catalog statistics
    true
}
```

**Restrictions:**
- Assumes uniform distribution (overestimates selectivity for skewed data)
- Assumes predicate independence (underestimates correlated predicates)
- Range predicates assume linear interpolation between min and max
- Default selectivity (1/3 for ranges, 1/10 for unknown) can be far off
- No handling of NULL semantics in original formulation

## Cost Model

```rust
fn estimation_accuracy(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    // Accuracy depends on data distribution
    if stats.data_is_uniform {
        0.95 // Very accurate for uniform data
    } else if stats.has_histograms {
        0.5 // Should use histograms instead
    } else if stats.high_skew {
        0.3 // Poor for skewed data
    } else {
        0.7 // Reasonable for moderate distributions
    }
}
```

**Assumptions:**
- Uniform distribution within each column
- Statistical independence between columns
- No correlation between predicates
- Known min/max values for range predicates

**Known failure modes:**
- Correlated columns: AND of correlated predicates overestimates selectivity
- Skewed distributions: equality on popular value is much less selective than 1/ICARD
- Join selectivity: 1/max(ICARD) ignores containment and skew

## Test Cases

### Positive: Equality predicate estimation

```sql
-- Table: employees, 10,000 rows
-- department has 50 distinct values
SELECT * FROM employees WHERE department = 'Engineering';

-- System R estimate: 1/50 = 2% = 200 rows
-- Actual (if uniform): 200 rows -- exact!
-- Actual (if skewed): could be 50-2000 rows
```

### Positive: AND of independent predicates

```sql
-- employees: 10,000 rows
-- department: 50 distinct, region: 10 distinct
SELECT * FROM employees
WHERE department = 'Engineering' AND region = 'West';

-- F(dept) = 1/50 = 0.02
-- F(region) = 1/10 = 0.1
-- F(AND) = 0.02 * 0.1 = 0.002 = 20 rows
-- Accurate if department and region are independent
```

### Negative: Correlated predicates (independence assumption fails)

```sql
-- city and zip_code are highly correlated
SELECT * FROM addresses
WHERE city = 'San Francisco' AND zip_code = '94105';

-- System R: F(city) * F(zip) = 1/1000 * 1/10000 = 0.0000001
-- Actual: F(city AND zip) ~ F(zip) = 1/10000
-- Overestimates selectivity by 10x (underestimates result size by 10x)
```

### Positive: Range predicate with known bounds

```sql
-- salary: min=30000, max=300000
SELECT * FROM employees WHERE salary > 200000;

-- F = (300000 - 200000) / (300000 - 30000) = 100000/270000 = 0.37
-- 37% of rows, ~3700 of 10000 rows
```

### Positive: Join selectivity estimation

```sql
-- orders: 1M rows, customer_id has 100K distinct values
-- customers: 100K rows, id has 100K distinct values
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;

-- Join selectivity: 1/max(100K, 100K) = 1/100K = 0.00001
-- Expected output: 1M * 100K * 0.00001 = 1M rows
-- Correct: each order matches exactly one customer
```

## References

**Original paper:**
- Selinger, P. Griffiths, et al., "Access Path Selection in a Relational Database Management System", ACM SIGMOD 1979
  - DOI: 10.1145/582095.582099
  - Section 3: "The Research Storage System (RSS) -- statistics"
  - Table in Section 4: "Selectivity factors" (the F formulas)

**Improvements to selectivity estimation:**
- Ioannidis, Y.E., Christodoulakis, S., "On the Propagation of Errors in the Size of Join Results", ACM SIGMOD 1991
  - DOI: 10.1145/115790.115835
  - Analysis of estimation errors and their propagation

- Poosala, V., et al., "Improved Histograms for Selectivity Estimation", ACM SIGMOD 1996
  - DOI: 10.1145/233269.233342
  - Equi-depth, equi-width, and V-optimal histograms

- Markl, V., et al., "Consistent Selectivity Estimation via Maximum Entropy", VLDB 2007
  - DOI: 10.14778/1325851.1325866
  - Addressing the independence assumption

**Implementation in databases:**
- PostgreSQL: `src/backend/utils/adt/selfuncs.c` - all selectivity functions
- MySQL: `sql/opt_range.cc` - range selectivity
- Every database uses these formulas as the baseline

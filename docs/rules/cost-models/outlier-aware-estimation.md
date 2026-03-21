# Rule: Outlier-Aware Cost Estimation

**Category:** cost-models
**File:** `rules/cost-models/outlier-aware-estimation.rra`

## Metadata

- **ID:** `outlier-aware-estimation`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, duckdb, clickhouse, mssql
- **Tags:** cost, outlier, skew, heavy-hitter, robust-estimation
- **Authors:** "RA Contributors"


# Outlier-Aware Cost Estimation

## Metadata
- **Rule ID**: `outlier-aware-estimation`
- **Category**: Cost Models
- **Complexity**: O(n) for statistics collection, O(1) per estimation query
- **Introduced**: PostgreSQL MCV lists, Oracle frequency histograms
- **Prerequisites**: Column statistics with value-frequency tracking
- **Alternatives**: histogram-based-estimation, sampling-based-estimation

## Description

Outlier-aware cost estimation explicitly models data skew -- situations
where a small number of values account for a disproportionate fraction
of rows. Standard histogram-based estimation assumes uniform distribution
within each bucket, which breaks down for heavy hitters (values with
extremely high frequency) and long-tail distributions.

The key insight: separate outlier values from the bulk distribution and
estimate them independently. PostgreSQL's Most Common Values (MCV) list
stores the top-N most frequent values with their exact frequencies. Values
not in the MCV list are estimated using histograms over the remaining
distribution.

**When to use:**
- Columns with Zipf or power-law distributions (user IDs, IP addresses)
- Status columns where one value dominates (99% 'active', 1% other)
- Foreign key columns with skewed reference patterns
- Any column where top-K values have significantly higher frequency

**Advantages:**
- Exact selectivity for most-queried values (MCVs)
- Robust against extreme skew that breaks histogram accuracy
- Low storage overhead (MCV list is compact)
- Directly improves join cardinality for skewed keys

**Disadvantages:**
- MCV list has fixed size (PostgreSQL default: 100 values)
- Values just below the MCV threshold may still be poorly estimated
- MCV lists become stale as data changes
- Does not handle multi-column skew (use joint MCV for that)

## Formal Model

```
Given column C with values {v1, v2, ..., vd}:

MCV list: {(v_i, freq_i) | freq_i >= threshold, i = 1..k}
  where k = MCV list size (e.g., 100)

For predicate P on column C:
  if value in MCV list:
    selectivity = mcv_frequency(value)
  else:
    remaining_rows = total_rows * (1 - sum(mcv_frequencies))
    remaining_ndv = total_ndv - k
    selectivity = (1 - sum(mcv_frequencies)) / remaining_ndv

For range predicates:
  sel = sum(freq_i for v_i in range AND v_i in MCV)
      + histogram_estimate(range, excluding MCV values)
```

## Implementation (egg rewrite rules)

```lisp
;; Use MCV for exact value lookup
(rewrite (selectivity (= ?col ?val) ?table)
  (mcv-lookup ?table ?col ?val)
  :if (in-mcv-list ?table ?col ?val))

;; Use residual histogram for non-MCV values
(rewrite (selectivity (= ?col ?val) ?table)
  (residual-histogram-selectivity ?table ?col ?val)
  :if (not (in-mcv-list ?table ?col ?val))
  :if (has-histogram ?table ?col))

;; Range selectivity: MCV + histogram
(rewrite (selectivity (between ?col ?lo ?hi) ?table)
  (+ (mcv-range-sum ?table ?col ?lo ?hi)
     (residual-histogram-range ?table ?col ?lo ?hi)))

;; Detect and handle heavy hitters in join estimation
(rewrite (join-cardinality ?left ?right ?key)
  (skew-aware-join-cardinality ?left ?right ?key)
  :if (is-skewed ?left ?key)
  :if (> (skew-factor ?left ?key) 5.0))
```

## Implementation Pattern

```rust
pub struct OutlierAwareEstimator {
    mcv_list: Vec<(Value, f64)>,  // (value, frequency)
    mcv_total_freq: f64,
    histogram: EquiDepthHistogram,  // Over non-MCV values
    total_ndv: u64,
    null_fraction: f64,
}

impl OutlierAwareEstimator {
    pub fn estimate_equality(
        &self,
        value: &Value,
    ) -> f64 {
        // Check MCV list first
        for (mcv_val, freq) in &self.mcv_list {
            if mcv_val == value {
                return *freq; // Exact frequency
            }
        }

        // Not in MCV: use residual distribution
        let remaining_ndv = self.total_ndv as f64
            - self.mcv_list.len() as f64;
        if remaining_ndv <= 0.0 {
            return 0.0;
        }

        (1.0 - self.mcv_total_freq - self.null_fraction)
            / remaining_ndv
    }

    pub fn estimate_range(
        &self,
        low: &Value,
        high: &Value,
    ) -> f64 {
        // Sum MCV values in range
        let mcv_contribution: f64 = self.mcv_list.iter()
            .filter(|(v, _)| v >= low && v <= high)
            .map(|(_, freq)| freq)
            .sum();

        // Add histogram contribution for non-MCV values
        let hist_contribution = self.histogram
            .range_selectivity(low, high)
            * (1.0 - self.mcv_total_freq);

        mcv_contribution + hist_contribution
    }
}

/// Skew-aware join cardinality estimation
pub fn skew_aware_join_cardinality(
    left: &OutlierAwareEstimator,
    right: &OutlierAwareEstimator,
    left_card: u64,
    right_card: u64,
) -> u64 {
    let mut total = 0.0;

    // MCV-MCV matches: exact cardinality
    for (lv, lf) in &left.mcv_list {
        for (rv, rf) in &right.mcv_list {
            if lv == rv {
                total += (left_card as f64 * lf)
                    * (right_card as f64 * rf);
            }
        }
    }

    // MCV-residual matches
    for (lv, lf) in &left.mcv_list {
        let right_freq = right.estimate_equality(lv);
        if right_freq > 0.0 && !right.mcv_list.iter().any(|(v, _)| v == lv) {
            total += (left_card as f64 * lf)
                * (right_card as f64 * right_freq);
        }
    }

    // Residual-residual: uniform assumption on remaining
    let left_residual = 1.0 - left.mcv_total_freq;
    let right_residual = 1.0 - right.mcv_total_freq;
    let residual_ndv = (left.total_ndv - left.mcv_list.len() as u64)
        .max(1);
    total += left_card as f64 * left_residual
        * right_card as f64 * right_residual
        / residual_ndv as f64;

    total as u64
}
```

## Skew Detection

```rust
pub fn detect_skew(
    frequencies: &[f64],
    threshold: f64,
) -> SkewProfile {
    let n = frequencies.len() as f64;
    let mean = frequencies.iter().sum::<f64>() / n;

    // Coefficient of variation
    let variance = frequencies.iter()
        .map(|f| (f - mean).powi(2))
        .sum::<f64>() / n;
    let cv = variance.sqrt() / mean;

    // Top-K concentration
    let mut sorted = frequencies.to_vec();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
    let top_10_pct = sorted.iter()
        .take((n * 0.1) as usize)
        .sum::<f64>();

    SkewProfile {
        coefficient_of_variation: cv,
        top_10_percent_share: top_10_pct,
        is_skewed: cv > threshold || top_10_pct > 0.5,
        zipf_exponent: estimate_zipf_exponent(frequencies),
    }
}
```

## Cost Model

```rust
pub fn estimation_error_comparison(
    actual_selectivities: &[(Value, f64)],
    mcv_size: usize,
    histogram_buckets: usize,
) -> (f64, f64) {
    // Sort by frequency to identify MCVs
    let mut sorted = actual_selectivities.to_vec();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Error with MCV + histogram
    let mcv_error: f64 = sorted.iter()
        .enumerate()
        .map(|(i, (_, actual))| {
            if i < mcv_size {
                0.0 // MCV: exact match
            } else {
                // Histogram: uniform within bucket
                let residual_avg = sorted[mcv_size..].iter()
                    .map(|(_, f)| f).sum::<f64>()
                    / (sorted.len() - mcv_size) as f64;
                (residual_avg - actual).abs() / actual.max(1e-10)
            }
        })
        .sum::<f64>() / sorted.len() as f64;

    // Error with uniform assumption only
    let uniform = 1.0 / sorted.len() as f64;
    let uniform_error: f64 = sorted.iter()
        .map(|(_, actual)| {
            (uniform - actual).abs() / actual.max(1e-10)
        })
        .sum::<f64>() / sorted.len() as f64;

    (mcv_error, uniform_error)
}
```

## Test Cases

### Test 1: Heavy hitter in status column
```sql
CREATE TABLE accounts (
    id INT, status TEXT, balance DECIMAL
);
-- status: 'active' 95%, 'suspended' 3%, 'closed' 2%

SELECT COUNT(*) FROM accounts WHERE status = 'active';

-- Without MCV: uniform = 1/3 = 33% (1M * 0.33 = 330K)
-- With MCV: exact = 95% (1M * 0.95 = 950K)
-- Error without: 2.9x underestimate
```

### Test 2: Zipf distribution on user_id
```sql
CREATE TABLE page_views (
    user_id INT, page TEXT, timestamp TIMESTAMP
);
-- Top 1% of users generate 30% of page views

SELECT COUNT(*) FROM page_views WHERE user_id = 12345;

-- User 12345 is a power user (0.5% of all views)
-- Without MCV: uniform = 1/100K = 0.001% -> 10 rows
-- With MCV: exact = 0.5% -> 5000 rows
-- Error without: 500x underestimate
-- This causes nested-loop join instead of hash join
```

### Test 3: Skew-aware join estimation
```sql
-- orders.customer_id: top customer has 5% of all orders
-- customers.id: uniform (1 row per customer)

SELECT * FROM orders o JOIN customers c
ON o.customer_id = c.id
WHERE c.region = 'premium';

-- Standard join estimate: |orders| * |premium_customers| / NDV
-- Skew-aware: adjusts for high-volume premium customers
-- Standard: 10M * 100 / 100K = 10K
-- Skew-aware: accounts for top customers -> 25K (more accurate)
```

### Test 4: Range predicate with outlier values
```sql
-- Column: transaction_amount
-- Most values $1-$100, but outliers at $10,000+

SELECT COUNT(*) FROM transactions
WHERE amount BETWEEN 50 AND 200;

-- Histogram may have wide bucket including outliers
-- MCV tracks common amounts ($9.99, $19.99, $49.99, $99.99)
-- MCV + residual histogram: accurate decomposition
```

### Test 5: Negative -- uniform distribution
```sql
-- UUID column: no skew, no outliers
SELECT COUNT(*) FROM events WHERE event_id = 'abc-123';

-- Uniform distribution: MCV adds no benefit
-- 1/NDV estimate is correct
-- MCV list wastes storage on equally-frequent values
```

## Performance Characteristics

| Distribution | Uniform Estimate Error | MCV+Histogram Error |
|-------------|----------------------|---------------------|
| Uniform | < 1.5x | < 1.5x (no benefit) |
| Moderate skew | 2-5x | < 1.5x |
| Zipf (alpha=1) | 10-100x | < 2x |
| Extreme skew (99/1) | 50-500x | < 1.2x |

## References

1. **PostgreSQL**: Most Common Values and histograms
   - https://www.postgresql.org/docs/current/planner-stats-details.html

2. **Ioannidis & Poosala**: "Histogram-Based Approximation of Set-Valued Query-Answers"
   - VLDB 1999, combining histograms with frequency tracking

3. **Chaudhuri & Narasayya**: "Self-Tuning Database Systems: A Decade of Progress"
   - VLDB 2007, adaptive statistics for skewed distributions

4. **Cormode & Muthukrishnan**: "An Improved Data Stream Summary: The Count-Min Sketch"
   - LATIN 2005, streaming heavy hitter detection

5. **Oracle Frequency Histograms**:
   - https://docs.oracle.com/en/database/oracle/oracle-database/19/tgsql/optimizer-statistics.html

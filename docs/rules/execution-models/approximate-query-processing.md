# Rule: Approximate Query Processing (AQP)

**Category:** execution-models/experimental
**File:** `rules/execution-models/experimental/approximate-query-processing.rra`

## Metadata

- **ID:** `approximate-query-processing`
- **Version:** "1.0.0"
- **Databases:** spark, trino, clickhouse, snowflake, verdictdb
- **Tags:** execution, experimental, research, sampling, approximation, confidence, synopses
- **Authors:** Sameer Agarwal, Barzan Mozafari, Surajit Chaudhuri


# Approximate Query Processing (AQP)

## Description

Approximate query processing trades exact answers for dramatically faster
response times by computing results on a sample of the data rather than the
full dataset. The system returns an approximate answer along with statistical
error bounds (confidence intervals), enabling users to make decisions quickly
on massive datasets where exact answers would require minutes or hours.

**When to apply**: Exploratory analytics, dashboards, and interactive data
exploration where approximate answers with known error bounds are acceptable.
Particularly valuable for aggregate queries (COUNT, SUM, AVG) on very large
tables where users prioritize response time over precision.

**Why it works**: For aggregate queries, the Central Limit Theorem guarantees
that sample-based estimates converge to the true answer at rate O(1/sqrt(n))
where n is the sample size. A 1% sample of 1 billion rows (10 million samples)
provides 95% confidence intervals within ~1% of the true answer for most
aggregates. Processing 10M rows instead of 1B gives a 100x speedup.

**Key techniques:**
- **Uniform random sampling**: Simple random sample of rows. Works well for
  COUNT, SUM, AVG on non-skewed data.
- **Stratified sampling**: Sample proportionally from each stratum (group).
  Ensures rare groups are represented. Essential for GROUP BY queries.
- **Bernoulli sampling**: Each row independently included with probability p.
  Easy to implement, slight variance in sample size.
- **Reservoir sampling**: Fixed-size sample from a stream. O(k) memory for
  k-element sample.
- **Sample synopses**: Pre-computed samples stored alongside the table.
  Avoids runtime sampling overhead.
- **Online aggregation**: Continuously refine the estimate as more data is
  processed. User can stop when accuracy is sufficient.

**Error estimation:**
- **Closed-form**: For SUM/AVG, use CLT: error = z * std / sqrt(n)
- **Bootstrap**: Resample the sample to estimate error for complex aggregates
- **Analytical bounds**: Hoeffding/Chernoff bounds for worst-case guarantees

## Relational Algebra

```algebra
-- Exact query:
result = Aggregate(Scan(R), SUM(col))
  -- Cost: O(|R|)

-- Approximate query with sampling:
sample = Sample(R, rate=0.01)  -- 1% sample
approx_result = Aggregate(sample, SUM(col))
scaled_result = approx_result / 0.01  -- scale to full table
error = z_alpha * StdDev(sample.col) / sqrt(|sample|)
  -- Cost: O(0.01 * |R|) = O(|R| / 100)

-- Stratified sampling for GROUP BY:
for each group g in R:
  sample_g = Sample(R[group=g], rate=max(0.01, k/|R[g]|))
  result_g = Aggregate(sample_g, SUM(col)) / sample_rate_g
  error_g = z * StdDev(sample_g.col) / sqrt(|sample_g|)
  -- Ensures each group has at least k samples
```

## Implementation

```rust
/// Approximate query processing engine
pub struct AQPEngine {
    /// Pre-computed sample synopses
    synopses: HashMap<TableId, Vec<SampleSynopsis>>,
    /// Default confidence level (e.g., 0.95)
    confidence_level: f64,
    /// Default relative error target (e.g., 0.05)
    error_target: f64,
}

/// Pre-computed sample stored alongside the table
pub struct SampleSynopsis {
    /// Sampling method used
    method: SamplingMethod,
    /// Sampling rate (fraction of original table)
    rate: f64,
    /// Number of rows in sample
    sample_size: usize,
    /// Stratification column (if stratified)
    strata_column: Option<ColumnId>,
    /// The actual sample data
    data: Vec<Row>,
    /// Per-stratum sample rates (for stratified)
    strata_rates: HashMap<Value, f64>,
}

pub enum SamplingMethod {
    UniformRandom,
    Stratified { strata_col: ColumnId },
    Bernoulli { prob: f64 },
    Reservoir { capacity: usize },
}

impl AQPEngine {
    /// Execute aggregate query approximately
    pub fn approximate_aggregate(
        &self,
        table: &TableId,
        agg: AggregateType,
        target_col: ColumnId,
        filter: Option<&Predicate>,
        group_by: Option<&[ColumnId]>,
    ) -> ApproximateResult {
        // Select best synopsis for this query
        let synopsis = self.select_synopsis(
            table, group_by,
        );

        // Apply filter to sample
        let filtered: Vec<&Row> = synopsis.data.iter()
            .filter(|row| match filter {
                Some(pred) => evaluate(pred, row),
                None => true,
            })
            .collect();

        match group_by {
            None => self.ungrouped_aggregate(
                &filtered, agg, target_col, synopsis,
            ),
            Some(cols) => self.grouped_aggregate(
                &filtered, agg, target_col,
                cols, synopsis,
            ),
        }
    }

    /// Compute aggregate with error bounds
    fn ungrouped_aggregate(
        &self,
        sample: &[&Row],
        agg: AggregateType,
        col: ColumnId,
        synopsis: &SampleSynopsis,
    ) -> ApproximateResult {
        let n = sample.len() as f64;
        let values: Vec<f64> = sample.iter()
            .map(|r| r.get_f64(col))
            .collect();

        let (estimate, error) = match agg {
            AggregateType::Count => {
                let count = n / synopsis.rate;
                let se = (n * (1.0 - synopsis.rate))
                    .sqrt() / synopsis.rate;
                (count, se)
            }
            AggregateType::Sum => {
                let sample_sum: f64 = values.iter().sum();
                let scaled = sample_sum / synopsis.rate;
                let variance: f64 = values.iter()
                    .map(|v| (v - sample_sum / n).powi(2))
                    .sum::<f64>() / (n - 1.0);
                let se = (variance * n).sqrt()
                    / synopsis.rate;
                (scaled, se)
            }
            AggregateType::Avg => {
                let mean = values.iter().sum::<f64>() / n;
                let variance = values.iter()
                    .map(|v| (v - mean).powi(2))
                    .sum::<f64>() / (n - 1.0);
                let se = (variance / n).sqrt();
                (mean, se)
            }
            AggregateType::CountDistinct => {
                // Use HyperLogLog on sample
                let hll = HyperLogLog::new(14);
                for v in &values {
                    hll.insert(*v as u64);
                }
                let estimate = hll.count() as f64;
                let se = estimate * 0.01; // ~1% error
                (estimate, se)
            }
        };

        let z = z_score(self.confidence_level);
        let ci_low = estimate - z * error;
        let ci_high = estimate + z * error;

        ApproximateResult {
            estimate,
            confidence_interval: (ci_low, ci_high),
            confidence_level: self.confidence_level,
            relative_error: error / estimate.abs(),
            sample_size: sample.len(),
        }
    }

    /// Determine minimum sample size for target accuracy
    pub fn required_sample_size(
        &self,
        table_size: u64,
        target_relative_error: f64,
        confidence_level: f64,
        estimated_cv: f64,  // coefficient of variation
    ) -> usize {
        let z = z_score(confidence_level);
        // n >= (z * CV / error)^2
        let n = (z * estimated_cv / target_relative_error)
            .powi(2);
        (n as usize).min(table_size as usize)
    }

    /// Select best pre-computed synopsis
    fn select_synopsis(
        &self,
        table: &TableId,
        group_by: Option<&[ColumnId]>,
    ) -> &SampleSynopsis {
        let synopses = &self.synopses[table];

        // Prefer stratified sample matching GROUP BY
        if let Some(cols) = group_by {
            for s in synopses {
                if let Some(sc) = &s.strata_column {
                    if cols.contains(sc) {
                        return s;
                    }
                }
            }
        }

        // Fall back to largest uniform sample
        synopses.iter()
            .max_by_key(|s| s.sample_size)
            .expect("no synopsis for table")
    }
}

/// Online aggregation: progressive refinement
pub struct OnlineAggregator {
    /// Running statistics
    running_sum: f64,
    running_sum_sq: f64,
    count: u64,
    total_rows: u64,
    /// Update interval for user display
    update_interval: u64,
}

impl OnlineAggregator {
    /// Process next row, return current estimate
    pub fn process_row(
        &mut self,
        value: f64,
    ) -> Option<ApproximateResult> {
        self.running_sum += value;
        self.running_sum_sq += value * value;
        self.count += 1;

        if self.count % self.update_interval != 0 {
            return None;
        }

        let n = self.count as f64;
        let fraction = n / self.total_rows as f64;

        // Current estimate
        let mean = self.running_sum / n;
        let variance = (self.running_sum_sq / n
            - mean * mean) * n / (n - 1.0);
        let se = (variance / n).sqrt();
        let z = z_score(0.95);

        Some(ApproximateResult {
            estimate: mean,
            confidence_interval: (
                mean - z * se,
                mean + z * se,
            ),
            confidence_level: 0.95,
            relative_error: se / mean.abs(),
            sample_size: self.count as usize,
        })
    }

    /// Check if target accuracy reached
    pub fn accuracy_reached(
        &self,
        target_error: f64,
    ) -> bool {
        if self.count < 100 {
            return false;
        }
        let n = self.count as f64;
        let mean = self.running_sum / n;
        let variance = (self.running_sum_sq / n
            - mean * mean) * n / (n - 1.0);
        let se = (variance / n).sqrt();
        let relative_error = se / mean.abs();
        relative_error < target_error
    }
}

fn z_score(confidence: f64) -> f64 {
    match confidence {
        c if c >= 0.99 => 2.576,
        c if c >= 0.95 => 1.96,
        c if c >= 0.90 => 1.645,
        _ => 1.28,
    }
}
```

**Restrictions:**
- Not suitable for exact results (WHERE id = X)
- Rare group problem: small groups may have zero samples
- Non-aggregation queries (ORDER BY, LIMIT) need different techniques
- DISTINCT and MEDIAN are harder to approximate than SUM/AVG
- Joins with sampling require careful treatment to avoid bias
- User trust: some applications require exact answers

## Cost Model

```rust
fn aqp_cost(
    table_size: u64,
    sample_rate: f64,
    target_error: f64,
    confidence: f64,
) -> AQPCostEstimate {
    let sample_size =
        (table_size as f64 * sample_rate) as u64;
    let z = z_score(confidence);

    // Execution cost
    let exact_ms = table_size as f64 * 10.0 / 1e6;
    let approx_ms = sample_size as f64 * 10.0 / 1e6;

    // Speedup
    let speedup = 1.0 / sample_rate;

    // Accuracy (CLT approximation for AVG)
    let estimated_error = z / (sample_size as f64).sqrt();

    AQPCostEstimate {
        exact_time_ms: exact_ms,
        approx_time_ms: approx_ms,
        speedup,
        estimated_relative_error: estimated_error,
        sample_size,
    }
}
```

**Typical performance:**
- 1% sample on 1B rows: 100x speedup, ~1% error for AVG/SUM
- 0.1% sample: 1000x speedup, ~3% error
- Online aggregation: interactive refinement in <1 second
- Stratified sampling: 10x better accuracy for GROUP BY on skewed data
- Break-even: AQP always faster; question is whether error is acceptable

## Test Cases

### Positive: Dashboard aggregate on massive table

```sql
SELECT country, AVG(spend), COUNT(*)
FROM transactions  -- 10 billion rows
GROUP BY country
WITH ERROR 0.05 CONFIDENCE 0.95;
-- Stratified 1% sample: 100M rows, ~10 seconds
-- Exact: 10B rows, ~15 minutes
-- Speedup: ~100x
-- Error: <5% for all countries with >10K transactions
```

### Positive: Exploratory analysis with early termination

```sql
-- Online aggregation: stream through data, refine estimate
SELECT AVG(price) FROM listings
WITH ONLINE AGGREGATION;
-- After 1% of data: estimate = $250K +/- 15%
-- After 5% of data: estimate = $247K +/- 5%
-- After 10% of data: estimate = $248K +/- 2%
-- User stops early: 10x speedup with 2% error
```

### Positive: Pre-computed synopsis for interactive exploration

```sql
-- Pre-compute 1% stratified sample on region
CREATE SAMPLE synopsis_1pct ON sales
  STRATIFIED BY region RATE 0.01;

-- Interactive queries use synopsis automatically
SELECT region, SUM(amount) FROM sales GROUP BY region;
-- Reads 1% sample: sub-second response
-- Confidence interval shown alongside each result
```

### Negative: Rare event counting

```sql
SELECT COUNT(*) FROM events WHERE type = 'CRITICAL';
-- 0.001% of events are CRITICAL
-- 1% sample: expected 0.0001 * 10M = 1 sample
-- Cannot reliably estimate from 1 sample
-- Error: potentially 100x+
-- Need targeted sampling or exact query for rare events
```

### Negative: Exact result required

```sql
SELECT balance FROM accounts WHERE id = 42;
-- Point lookup: sampling cannot help
-- Must return exact answer (financial correctness)
-- AQP not applicable for exact queries
```

### Negative: Highly skewed data

```sql
SELECT SUM(revenue) FROM companies;
-- Revenue: 0.1% of companies generate 80% of revenue
-- Uniform sample may miss top companies
-- Massive variance -> wide confidence intervals
-- Solution: stratified sampling by revenue tier
```

## References

**Academic papers:**
- Agarwal et al., "BlinkDB: Queries with Bounded Errors and Bounded Response Times on Very Large Data", EuroSys 2013
- Mozafari, Niu, "A Handbook for Building an Approximate Query Engine", IEEE Data Eng. Bull. 2015
- Hellerstein, Haas, Wang, "Online Aggregation", SIGMOD 1997
- Chaudhuri, Das, Narasayya, "Optimized Stratified Sampling for Approximate Query Processing", ACM TODS 2007
- Park et al., "VerdictDB: Universalizing Approximate Query Processing", SIGMOD 2018
- Li, Wu, "AQP++: Connecting Approximate Query Processing with Aggregate Precomputation", SIGMOD 2018

**Implementation:**
- Spark: TABLESAMPLE syntax, approximate aggregates
- ClickHouse: sample() table function
- Snowflake: TABLESAMPLE with Bernoulli/System sampling
- VerdictDB: Middleware for approximate query processing
- BlinkDB: AQP on Spark with error/latency bounds

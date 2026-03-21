# Rule: "Runtime Cost Model Calibration"

**Category:** cost-models
**File:** `rules/cost-models/cost-calibration.rra`

## Metadata

- **ID:** `cost-calibration`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, duckdb, cockroachdb, mssql
- **Tags:** cost, calibration, runtime, feedback, hardware, tuning
- **Authors:** "RA Contributors"


# Runtime Cost Model Calibration

## Description

Calibrates cost model parameters by running micro-benchmarks on the actual
hardware and comparing predicted costs against observed execution times.
Default cost parameters (seq_page_cost, random_page_cost, cpu_tuple_cost)
are set for generic hardware; calibration adjusts them for the specific
deployment environment.

Cost model calibration addresses the gap between theoretical cost units
and actual execution time. Without calibration, the optimizer may
overweight I/O on NVMe systems (where I/O is fast) or underweight CPU
on HDD systems (where I/O dominates). The calibration process runs
diagnostic queries and uses regression to fit cost parameters.

**When to apply**: When deploying on new hardware, after storage upgrades,
or when EXPLAIN ANALYZE consistently shows cost estimates diverging from
actual times.

**Why it works**: By measuring actual operation costs on the target hardware,
calibration aligns the cost model with reality. A calibrated model produces
cost estimates that are monotonically correlated with execution time,
which is sufficient for correct plan selection.

## Relational Algebra

```algebra
Calibration goal:
  Find parameters P = {seq_page_cost, random_page_cost, cpu_tuple_cost, ...}
  that minimize:
    sum_i (predicted_cost(query_i, P) - actual_time(query_i))^2

Calibration queries:
  Q1: Sequential scan (measures seq_page_cost)
  Q2: Random index scan (measures random_page_cost)
  Q3: CPU-only filter (measures cpu_tuple_cost)
  Q4: Hash join (measures hash overhead)
  Q5: Sort (measures comparison cost)
```

## Implementation

```rust
use std::time::Instant;

struct CostCalibrator {
    connection: DatabaseConnection,
    num_iterations: u32,
}

struct CalibratedParams {
    seq_page_cost: f64,
    random_page_cost: f64,
    cpu_tuple_cost: f64,
    cpu_index_tuple_cost: f64,
    cpu_operator_cost: f64,
    parallel_tuple_cost: f64,
    parallel_setup_cost: f64,
}

impl CostCalibrator {
    fn calibrate(&self) -> CalibratedParams {
        // Phase 1: Measure raw hardware speeds
        let seq_io_speed = self.measure_sequential_io();
        let rand_io_speed = self.measure_random_io();
        let cpu_speed = self.measure_cpu_throughput();

        // Phase 2: Run diagnostic queries
        let seq_scan_time = self.benchmark_sequential_scan();
        let index_scan_time = self.benchmark_index_scan();
        let filter_time = self.benchmark_filter();
        let hash_join_time = self.benchmark_hash_join();
        let sort_time = self.benchmark_sort();

        // Phase 3: Regression to fit parameters
        // Normalize to seq_page_cost = 1.0
        let seq_page_cost = 1.0;

        // Random I/O relative to sequential
        let random_page_cost = rand_io_speed.time_ms
            / seq_io_speed.time_ms;

        // CPU cost relative to sequential I/O
        let cpu_tuple_cost = cpu_speed.time_per_tuple_ms
            / seq_io_speed.time_per_page_ms;

        // Index tuple cost slightly higher (B-tree traversal)
        let cpu_index_tuple_cost = cpu_tuple_cost * 1.5;

        // Operator cost (predicate evaluation)
        let cpu_operator_cost = cpu_tuple_cost * 0.25;

        // Parallel overhead
        let parallel_tuple_cost = cpu_tuple_cost * 0.1;
        let parallel_setup_cost =
            self.measure_parallel_startup() / seq_io_speed.time_per_page_ms;

        CalibratedParams {
            seq_page_cost,
            random_page_cost,
            cpu_tuple_cost,
            cpu_index_tuple_cost,
            cpu_operator_cost,
            parallel_tuple_cost,
            parallel_setup_cost,
        }
    }

    fn measure_sequential_io(&self) -> IOBenchmark {
        // Read a large table sequentially, measure throughput
        let table = self.find_calibration_table(1_000_000);
        let mut times = Vec::new();

        for _ in 0..self.num_iterations {
            self.flush_caches();
            let start = Instant::now();
            self.connection.execute(&format!(
                "SELECT COUNT(*) FROM {}",
                table.name
            ));
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        let median_ms = median(&mut times);
        IOBenchmark {
            time_ms: median_ms,
            pages: table.pages as f64,
            time_per_page_ms: median_ms / table.pages as f64,
        }
    }

    fn measure_random_io(&self) -> IOBenchmark {
        // Random index lookups on unclustered index
        let table = self.find_calibration_table(1_000_000);
        let num_lookups = 10_000;
        let mut times = Vec::new();

        for _ in 0..self.num_iterations {
            self.flush_caches();
            let start = Instant::now();
            for _ in 0..num_lookups {
                self.connection.execute(&format!(
                    "SELECT * FROM {} WHERE id = {}",
                    table.name,
                    rand::random::<u64>() % table.row_count
                ));
            }
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        let median_ms = median(&mut times);
        IOBenchmark {
            time_ms: median_ms / num_lookups as f64,
            pages: 1.0,
            time_per_page_ms: median_ms / num_lookups as f64,
        }
    }

    fn measure_cpu_throughput(&self) -> CpuBenchmark {
        // In-memory filter to isolate CPU cost
        let table = self.find_cached_table(100_000);
        let mut times = Vec::new();

        for _ in 0..self.num_iterations {
            let start = Instant::now();
            self.connection.execute(&format!(
                "SELECT COUNT(*) FROM {} WHERE column1 + column2 > 0",
                table.name
            ));
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }

        let median_ms = median(&mut times);
        CpuBenchmark {
            time_per_tuple_ms: median_ms / table.row_count as f64,
        }
    }

    fn validate_calibration(
        &self,
        params: &CalibratedParams,
    ) -> ValidationResult {
        // Run a set of test queries and compare predicted vs actual
        let test_queries = self.generate_test_queries(20);
        let mut errors = Vec::new();

        for query in &test_queries {
            let predicted = self.predict_cost(query, params);
            let actual = self.measure_execution_time(query);

            let ratio = if predicted > actual {
                predicted / actual
            } else {
                actual / predicted
            };
            errors.push(ratio);
        }

        errors.sort_by(|a, b| a.partial_cmp(b).unwrap());

        ValidationResult {
            median_error: errors[errors.len() / 2],
            p90_error: errors[(errors.len() as f64 * 0.9) as usize],
            max_error: *errors.last().unwrap_or(&1.0),
            rank_correlation: self.spearman_correlation(
                &test_queries,
                params,
            ),
        }
    }

    fn spearman_correlation(
        &self,
        queries: &[Query],
        params: &CalibratedParams,
    ) -> f64 {
        // Rank correlation between predicted and actual costs
        // Values near 1.0 mean the model correctly ranks plans
        let mut predicted: Vec<f64> = queries
            .iter()
            .map(|q| self.predict_cost(q, params))
            .collect();
        let mut actual: Vec<f64> = queries
            .iter()
            .map(|q| self.measure_execution_time(q))
            .collect();

        let pred_ranks = rank_values(&predicted);
        let actual_ranks = rank_values(&actual);

        let n = pred_ranks.len() as f64;
        let d_squared_sum: f64 = pred_ranks
            .iter()
            .zip(actual_ranks.iter())
            .map(|(p, a)| (p - a) * (p - a))
            .sum();

        1.0 - (6.0 * d_squared_sum) / (n * (n * n - 1.0))
    }
}

struct IOBenchmark {
    time_ms: f64,
    pages: f64,
    time_per_page_ms: f64,
}

struct CpuBenchmark {
    time_per_tuple_ms: f64,
}

struct ValidationResult {
    median_error: f64,
    p90_error: f64,
    max_error: f64,
    rank_correlation: f64,
}
```

**Restrictions:**
- Calibration requires exclusive access (no concurrent workloads)
- Cache flushing may not be possible on all platforms
- Results vary with buffer pool warmth
- Parallel calibration depends on core count and scheduling
- Needs recalibration after hardware changes

## Cost Model

```rust
fn calibration_benefit(
    uncalibrated: &CalibratedParams,
    calibrated: &CalibratedParams,
    workload: &[Query],
) -> f64 {
    let mut total_uncalibrated_cost = 0.0;
    let mut total_calibrated_cost = 0.0;

    for query in workload {
        let uncal_plan = optimize_with(query, uncalibrated);
        let cal_plan = optimize_with(query, calibrated);

        total_uncalibrated_cost += execute_cost(uncal_plan);
        total_calibrated_cost += execute_cost(cal_plan);
    }

    (total_uncalibrated_cost - total_calibrated_cost)
        / total_uncalibrated_cost
}
```

**Typical benefit**: 20-60% improvement in plan quality when hardware
differs significantly from default assumptions (e.g., NVMe vs HDD defaults).

## Test Cases

### Test 1: HDD vs SSD random_page_cost

```sql
-- Default: random_page_cost = 4.0 (HDD assumption)
-- Calibrated SSD: random_page_cost = 1.1
-- Impact: index scans become cheaper, more queries use indexes

-- Query: SELECT * FROM orders WHERE total > 1000;
-- HDD model (rpc=4.0): prefers sequential scan
-- SSD model (rpc=1.1): prefers index scan (3.6x cheaper random I/O)
```

### Test 2: NVMe calibration

```sql
-- Calibrated NVMe: random_page_cost = 1.05, cpu_tuple_cost = 0.01
-- CPU now dominates (I/O nearly free)
-- Hash join with small build side always preferred
```

### Test 3: Validation with rank correlation

```sql
-- 20 test queries, measure predicted vs actual ranking
-- Uncalibrated: Spearman rho = 0.65 (wrong plan ranking 35% of time)
-- Calibrated: Spearman rho = 0.92 (correct ranking 92% of time)
-- Plan quality directly tied to ranking accuracy
```

### Test 4: Parallel cost calibration

```sql
-- 16-core machine, parallel_tuple_cost calibration
-- Default: parallel_tuple_cost = 0.1 (too expensive)
-- Calibrated: parallel_tuple_cost = 0.02 (parallelism cheap)
-- More queries use parallel plans after calibration
```

## References

**Cost calibration:**
- Wu et al., "Predicting Query Execution Time: Are Optimizer Cost Models Really Unusable?", ICDE 2013
- Leis et al., "Query Optimization Through the Looking Glass", VLDB 2017

**Implementations:**
- PostgreSQL: `random_page_cost`, `cpu_tuple_cost` GUC parameters
- MySQL: cost model API (`opt_costmodel.h`)
- Oracle: `DBMS_STATS.GATHER_SYSTEM_STATS` for system statistics
- mssql: auto-tuning with Query Store

**Feedback-driven calibration:**
- Stillger et al., "LEO - DB2's LEarning Optimizer", VLDB 2001
- Chaudhuri, "Self-Tuning Database Systems", VLDB 2007

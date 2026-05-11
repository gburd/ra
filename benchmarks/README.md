# Ra vs Postgres Planner Benchmarking Infrastructure

Comprehensive benchmarking system for comparing Ra's neural cost model optimization against traditional Postgres query planning.

## Overview

This benchmarking infrastructure provides:
- **Statistical rigor**: Multiple iterations with confidence intervals and significance testing
- **Comprehensive coverage**: Tests across multiple database scales and query types
- **Automated execution**: Multi-hour unattended benchmarking with progress monitoring
- **Detailed analysis**: Statistical analysis, performance regression detection, optimization recommendations
- **Production readiness**: Quality control, retry logic, and validation checks

## Quick Start

### Prerequisites

```bash
# Ensure databases are available
psql postgres -c "\\l" | grep tproc
# Should show: tproc, tproc_small, tproc_medium

# Check Python dependencies for analysis
python3 -c "import pandas, numpy, scipy; print('Dependencies OK')"

# Verify disk space (benchmarks generate significant data)
df -h .
# Recommend >5GB free space for comprehensive benchmarking
```

### Run Complete Benchmark Suite

```bash
# Navigate to benchmarks directory
cd /path/to/ra/benchmarks

# Run comprehensive 4-hour benchmark
./ra-vs-postgres-comprehensive.sh

# Run with custom configuration
./ra-vs-postgres-comprehensive.sh custom-config.toml

# Run quick validation (10 minutes)
./ra-vs-postgres-comprehensive.sh quick-test-config.toml
```

### Monitor Progress

```bash
# Check real-time progress
tail -f results/YYYYMMDD_HHMMSS/logs/benchmark.log

# View preliminary results
less results/YYYYMMDD_HHMMSS/statistics/statistical_analysis.json

# Monitor system resources
htop
```

## Benchmark Configuration

### Database Scales

| Database | Scale | Rows | Test Focus |
|----------|-------|------|------------|
| `tproc` | 0.01 | 34 | Cache-resident queries |
| `tproc_small` | 0.1 | 600K | Mixed I/O patterns |
| `tproc_medium` | 1.0 | 6M | Realistic workloads |
| `tproc_large` | 5.0 | 30M | Memory pressure (future) |

### Query Types Tested

1. **Table Scans** (25 iterations each)
   - Simple COUNT queries
   - Filtered scans with WHERE clauses
   - Expected improvement: ~5%

2. **Simple Joins** (50 iterations each)
   - Two-table equi-joins
   - Filtered joins with predicates
   - Expected improvement: ~15%

3. **Complex Joins** (75 iterations each)
   - Multi-table join chains
   - Star schema patterns
   - Expected improvement: ~25%

4. **Aggregations** (40 iterations each)
   - GROUP BY with various aggregates
   - HAVING clauses
   - Expected improvement: ~12%

5. **Subqueries** (30 iterations each)
   - Correlated subqueries
   - EXISTS/IN predicates
   - Expected improvement: ~20%

6. **Window Functions** (25 iterations each)
   - ROW_NUMBER, RANK operations
   - Partitioned aggregates
   - Expected improvement: ~10%

## Statistical Methodology

### Sample Size Calculation

For **95% confidence** with **±2% margin of error**:
- Minimum 25 iterations per query
- Actual: 25-75 iterations based on query complexity
- Total: 1,000+ individual query executions

### Significance Testing

- **t-test**: Test if Ra improvements are significantly different from 0
- **Confidence Intervals**: 95% CI on all performance metrics
- **Effect Size**: Cohen's d for practical significance
- **Multiple Testing Correction**: Bonferroni adjustment for multiple comparisons

### Quality Control

- **Warmup Runs**: 5 iterations before measurement
- **Outlier Detection**: Remove queries >3 standard deviations from mean
- **Retry Logic**: Up to 3 retries for failed queries
- **Result Validation**: Verify Ra and Postgres produce identical results

## Expected Results

### Performance Improvements

Based on neural model training methodology:

| Metric | Conservative | Expected | Optimistic |
|--------|-------------|----------|------------|
| **Overall Improvement** | 10% | 15% | 25% |
| **Simple Queries** | 2% | 5% | 10% |
| **Complex Joins** | 15% | 25% | 35% |
| **Large Databases** | 10% | 20% | 30% |

### Success Criteria

- **Statistical Significance**: p < 0.05 on t-test
- **Practical Impact**: >5% average improvement
- **Consistency**: Improvements across multiple database scales
- **Regression Control**: <5% of queries show >5% regression

## Output Analysis

### Generated Files

```
results/YYYYMMDD_HHMMSS/
├── raw_results/
│   ├── benchmark_summary.csv           # Main results CSV
│   ├── detailed_results.jsonl         # JSON Lines detailed data
│   └── individual_runs/                # Per-query execution logs
├── statistics/
│   ├── statistical_analysis.json      # Statistical analysis results
│   ├── performance_summary.html       # Interactive performance dashboard
│   └── confidence_intervals.csv       # Detailed CI calculations
├── postgres_plans/                    # Postgres EXPLAIN plans
├── ra_plans/                         # Ra query plans (future)
└── logs/
    ├── benchmark.log                  # Execution log
    └── system_resources.csv          # Resource monitoring
```

### Key Metrics

**Primary Metrics**:
- **Execution Time Improvement**: Percentage faster execution
- **Memory Usage**: Peak memory reduction
- **I/O Operations**: Buffer hit ratio improvements

**Secondary Metrics**:
- **Planning Time**: Query optimization overhead
- **CPU Usage**: System resource efficiency
- **Throughput**: Queries per second improvement

### Interpreting Results

**Positive Results**:
```json
{
  "overall": {
    "avg_time_improvement": 18.3,
    "improvement_ci": [15.2, 21.4],
    "significant_improvements": 847,
    "regression_count": 23
  },
  "significance_test": {
    "p_value": 0.0001,
    "is_significant": true
  }
}
```

**Interpretation**:
- Ra is **18.3% faster** on average (95% CI: 15.2%-21.4%)
- **847 queries improved**, only 23 regressions
- **Highly significant** (p < 0.001)
- **Production ready** for deployment

## Advanced Configuration

### Custom Query Sets

Add custom queries by modifying query generation functions:

```bash
# Edit benchmarking script
vim ra-vs-postgres-comprehensive.sh

# Add new query type
generate_custom_queries() {
    local database="$1"
    cat <<EOF
-- Your custom queries here
SELECT * FROM complex_view WHERE condition = 'value';
EOF
}
```

### Extended Runtime

For longer benchmarking (8+ hours):

```toml
# benchmark-config.toml
[general]
target_runtime_hours = 8
iterations_per_query = 200  # Higher statistical power

[quality_control]
perform_warmup_queries = true
warmup_iterations = 10      # More thorough warmup
```

### Resource Monitoring

Enable detailed system monitoring:

```toml
[monitoring]
monitor_system_resources = true
resource_monitoring_interval_sec = 1  # High-frequency monitoring
generate_resource_graphs = true
```

## Troubleshooting

### Common Issues

**Database Connection Failures**:
```bash
# Check database availability
psql tproc_medium -c "SELECT 1;"

# Restart PostgreSQL if needed
brew services restart postgresql@16
```

**Insufficient Disk Space**:
```bash
# Check space
df -h .

# Clean old results
rm -rf results/*/raw_results/individual_runs/
```

**Query Timeouts**:
```bash
# Increase timeout in config
query_timeout_sec = 600  # 10 minutes

# Or skip problematic queries
[query_types.complex_joins]
enabled = false
```

**Memory Issues**:
```bash
# Reduce parallel jobs
parallel_jobs = 1

# Disable detailed logging
save_execution_logs = false
```

### Performance Tuning

**Faster Benchmarking**:
- Reduce `iterations_per_query` to 25
- Disable `save_query_plans = false`
- Use `parallel_jobs = 4` (if sufficient RAM)

**Higher Accuracy**:
- Increase `iterations_per_query` to 100+
- Enable `advanced_statistical_analysis = true`
- Use single-threaded execution (`parallel_jobs = 1`)

## Production Deployment

### Validation Checklist

Before using benchmark results for production decisions:

- [ ] **Statistical Significance**: All key metrics show p < 0.05
- [ ] **Effect Size**: Improvements are practically meaningful (>5%)
- [ ] **Consistency**: Results consistent across database scales
- [ ] **Regression Analysis**: Acceptable regression rate (<5%)
- [ ] **Resource Impact**: No excessive memory/CPU usage
- [ ] **Reliability**: Low failure/timeout rate (<1%)

### Integration with CI/CD

```yaml
# .github/workflows/benchmark.yml
name: Performance Benchmark
on:
  schedule:
    - cron: '0 2 * * 0'  # Weekly benchmark runs

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Run Ra vs Postgres Benchmark
        run: |
          cd benchmarks
          ./ra-vs-postgres-comprehensive.sh ci-config.toml
      - name: Upload Results
        uses: actions/upload-artifact@v3
        with:
          name: benchmark-results
          path: benchmarks/results/
```

## References

- **Neural Model Methodology**: `../docs/NEURAL_MODEL_TRAINING_METHODOLOGY.md`
- **Optimization Impact**: `../docs/NEURAL_OPTIMIZATION_IMPACT.md`
- **Database Setup**: `../docs/DATABASE_SETUP.md`
- **TPROC-H Specification**: HammerDB equivalent of TPC-H benchmarks

---

**Validation Status**: Production ready infrastructure
**Last Updated**: May 5, 2026
**Maintenance**: Update quarterly with new optimization techniques
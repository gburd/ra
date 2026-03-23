# RFC 0015: Configuration Auto-Tuning

- **Status:** Under Review
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

An auto-tuning system that analyzes database workloads and hardware
profiles to recommend optimal PostgreSQL configuration parameters
(work_mem, effective_cache_size, shared_buffers, etc.) and RA
optimizer settings. The system uses RA's cost model to predict the
impact of configuration changes on query performance.

## Motivation

PostgreSQL ships with conservative default configuration values
designed for minimal hardware. Production deployments require
tuning dozens of parameters, and the optimal values depend on:

- Hardware characteristics (memory, storage, CPU cores)
- Workload profile (OLTP, OLAP, mixed)
- Data size and growth rate
- Concurrency level

Manual tuning requires deep expertise. Existing tools (PGTune)
provide static recommendations based on hardware alone. RA can
do better by combining hardware awareness (RFC 0005) with
workload analysis.

## Guide-Level Explanation

### Basic Usage

```bash
# Analyze and recommend configuration
ra-cli tune --database postgres://localhost/mydb

# Analyze with specific hardware profile
ra-cli tune --hardware "aws-r6g.2xlarge" \
  --database postgres://localhost/mydb

# Dry run: show recommendations without applying
ra-cli tune --dry-run --database postgres://localhost/mydb
```

### Output

```
CONFIGURATION RECOMMENDATIONS

Hardware: AWS r6g.2xlarge (8 vCPU, 64GB RAM, gp3 EBS)
Workload: Mixed OLTP/OLAP (70/30)
Connections: avg 45, peak 120

Parameter                Current    Recommended  Impact
---------------------------------------------------------
shared_buffers           128MB      16GB         +35% cache hits
effective_cache_size     4GB        48GB         +12% cost accuracy
work_mem                 4MB        64MB         +8x sort performance
maintenance_work_mem     64MB       2GB          +4x VACUUM speed
max_parallel_workers     2          6            +3x parallel scans
random_page_cost         4.0        1.1          Better index selection

RA Optimizer Settings:
  ra_planner.rules       all        oltp_focused  -40% planning time
  ra_planner.cost_model  generic    aws_nvme      +15% plan accuracy

Estimated impact: +28% overall query throughput
```

### Configuration Profiles

```bash
# Generate a profile for a specific workload type
ra-cli tune --profile oltp --hardware "bare-metal-nvme"
ra-cli tune --profile olap --hardware "aws-i3.4xlarge"
ra-cli tune --profile mixed --hardware "gcp-n2-standard-8"
```

## Reference-Level Explanation

### Architecture

```
Database Introspection
  |-- pg_settings (current configuration)
  |-- pg_stat_statements (workload queries)
  |-- Hardware detection (or user-provided profile)
  |
  v
Workload Classification
  |-- OLTP score (short queries, high concurrency, point lookups)
  |-- OLAP score (complex queries, low concurrency, full scans)
  |-- Mixed ratio
  |
  v
Parameter Optimization
  |-- For each tunable parameter:
  |   |-- Generate candidate values
  |   |-- Estimate workload cost with RA optimizer
  |   |-- Select value minimizing total workload cost
  |
  v
Validation
  |-- Check parameter interactions (e.g., shared_buffers + work_mem
  |   must fit in available memory)
  |-- Verify settings are safe (no OOM risk)
  |
  v
Recommendations
```

### Tunable Parameters

| Parameter | Range | Selection Basis |
|-----------|-------|-----------------|
| shared_buffers | 25-40% of RAM | Working set size |
| effective_cache_size | 50-75% of RAM | Total cache estimate |
| work_mem | 4MB-256MB | Sort/hash operation sizes |
| maintenance_work_mem | 64MB-4GB | Vacuum/index build needs |
| max_parallel_workers_per_gather | 0-cores/2 | Query parallelism benefit |
| random_page_cost | 1.0-4.0 | Storage characteristics |
| seq_page_cost | 1.0 | Baseline (usually unchanged) |
| default_statistics_target | 100-1000 | Cardinality estimation needs |

### Cost-Based Selection

For each parameter, the tuner:

1. Generates 5-10 candidate values across the valid range
2. For each value, re-runs the RA optimizer on the workload
3. Computes total workload cost (sum of query costs weighted by
   frequency)
4. Selects the value minimizing total cost

This is more accurate than static formulas because it accounts for
the actual queries being run.

### Safety Checks

The tuner enforces safety constraints:

- `shared_buffers + max_connections * work_mem < 80% of RAM`
- `maintenance_work_mem <= 10% of RAM`
- `max_parallel_workers <= CPU cores`
- No parameter exceeds PostgreSQL's documented maximum

## Drawbacks

- Configuration impact is estimated, not measured -- actual results
  may differ from predictions
- Parameter interactions create a combinatorial search space; the
  greedy approach may miss global optima
- Recommendations assume stable workload; bursty or seasonal
  patterns may need different settings at different times
- Applying configuration changes requires PostgreSQL restart for
  some parameters

## Rationale and Alternatives

**Alternative: PGTune.** Simple hardware-based recommendations.
Fast and well-tested but ignores workload characteristics entirely.

**Alternative: pgtune + pg_stat_statements manual analysis.**
Current best practice for DBAs. Effective but requires expertise
and ongoing manual effort.

**Alternative: Machine learning auto-tuning (OtterTune-style).**
Train models on configuration-performance data. Requires a large
training dataset and may not generalize across hardware. RA's
cost model provides a physics-based alternative.

The cost-model approach was chosen because it combines RA's existing
optimizer infrastructure with hardware profiles, avoiding the need
for training data while producing workload-specific recommendations.

## Prior Art

- PGTune -- static PostgreSQL configuration calculator
- OtterTune (CMU) -- ML-based database tuning
- Oracle Automatic Memory Management
- MySQL Performance Schema + sys schema

## Unresolved Questions

- How to handle parameters that require restart vs reload?
- Should the system support gradual parameter changes (ramp up
  work_mem over time)?
- How to validate recommendations safely in production
  (canary approach)?
- Should RA optimizer settings be tuned independently or jointly
  with PostgreSQL parameters?

## Future Possibilities

- Continuous auto-tuning based on workload drift detection
- Per-query parameter overrides (SET LOCAL work_mem for specific
  queries)
- Multi-database fleet tuning with centralized configuration
  management
- Integration with Kubernetes operators for container resource
  limits
- Bayesian optimization for parameters that are expensive to
  evaluate

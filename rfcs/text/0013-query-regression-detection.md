# RFC 0013: Query Regression Detection

- **Status:** Under Review
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

A system for detecting query plan regressions -- cases where a
previously fast query becomes slow due to plan changes caused by
statistics updates, schema changes, or configuration drift. The
system maintains a baseline of known-good plans and alerts when
deviations occur.

## Motivation

Plan regressions are one of the most common and disruptive
production database issues. A query that ran in 50ms suddenly takes
30 seconds because the optimizer chose a different plan after an
`ANALYZE` updated cardinality estimates. These regressions are
hard to detect in advance and often discovered only through user
complaints.

Proactive regression detection allows DBAs to:

- Catch regressions before they impact users
- Understand why a plan changed
- Roll back to a known-good plan (via plan pinning or hints)
- Build confidence in statistics refresh and schema changes

## Guide-Level Explanation

### Baseline Capture

```bash
# Capture current plan baselines for monitored queries
ra-cli regression baseline --database postgres://localhost/mydb

# Capture baseline for a specific query
ra-cli regression baseline --query "SELECT * FROM orders WHERE ..."
```

### Detection

```bash
# Check for regressions against baseline
ra-cli regression check --database postgres://localhost/mydb

# Continuous monitoring mode
ra-cli regression watch --interval 300
```

### Output

```
REGRESSION DETECTED: query_hash=0x7a3f1b2c
  Baseline cost: 142.3 (Hash Join, Index Scan)
  Current cost:  8,291.0 (Nested Loop, Seq Scan)
  Regression factor: 58.3x
  Cause: statistics change on orders table
    row_count: 50,000 -> 5,000,000 (100x growth)
  Recommendation:
    1. Pin previous plan via pg_plan_advice
    2. Create index: CREATE INDEX ON orders(customer_id)
```

## Reference-Level Explanation

### Plan Fingerprinting

Each query plan is fingerprinted by its operator tree structure
(join types, scan methods, operator ordering) independent of
specific cost numbers. This allows detecting structural plan
changes even when costs shift slightly.

```rust
pub struct PlanFingerprint {
    pub operator_tree: Vec<OperatorNode>,
    pub join_order: Vec<(String, String)>,
    pub scan_methods: HashMap<String, ScanMethod>,
    pub hash: u64,
}
```

### Baseline Storage

Baselines are stored as TOML files (one per query):

```toml
[query]
hash = "0x7a3f1b2c"
sql = "SELECT * FROM orders WHERE ..."
captured_at = "2026-03-15T10:00:00Z"

[plan]
fingerprint = "0xabc123"
cost = 142.3
operators = ["HashJoin", "IndexScan(orders_idx)", "SeqScan(customers)"]

[statistics]
orders.row_count = 50000
customers.row_count = 10000
```

### Regression Analysis

When a regression is detected, the analyzer:

1. Compares the baseline and current plan fingerprints
2. Identifies which statistics changed
3. Runs the RA optimizer to find the optimal plan
4. Generates a root cause analysis and recommendations

### Sensitivity Scoring

Not all plan changes are regressions. The system computes a
sensitivity score based on:

- Cost ratio (current / baseline)
- Structural change magnitude (how different are the plans)
- Historical stability (how often has this query's plan changed)

Only changes exceeding a configurable threshold trigger alerts.

## Drawbacks

- Baseline maintenance overhead: baselines become stale and need
  periodic refresh
- False positives when workload patterns change legitimately
- Storing baselines for all monitored queries scales linearly with
  query count
- Requires a representative baseline period to capture normal
  variation

## Rationale and Alternatives

**Alternative: Plan pinning only.** Force specific plans without
detection. Simpler but brittle -- pinned plans become suboptimal
as data evolves.

**Alternative: Oracle-style SQL Plan Baselines.** Automatic plan
capture and evolution. More sophisticated but significantly more
complex. The detection-first approach lets DBAs make informed
decisions.

## Prior Art

- Oracle SQL Plan Baselines and SQL Plan Management
- SQL Server Plan Regression Detection (automatic plan correction)
- pg_store_plans -- stores execution plans in PostgreSQL
- Auto-Steer (Microsoft Research) -- learned plan steering

## Unresolved Questions

- How long should baselines be retained before refresh?
- Should the system support automatic plan rollback or always
  require human approval?
- How to handle parameterized queries with variable plan choices?

## Future Possibilities

- Machine learning models to predict regressions before they occur
- Integration with CI/CD to detect regressions from schema changes
  before deployment
- Cross-database regression detection in federated environments
- Automated plan evolution (accept better plans, reject worse ones)

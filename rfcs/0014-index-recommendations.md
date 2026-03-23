# RFC 0014: Automatic Index Recommendations

- **Status:** Under Review
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

An index recommendation engine that analyzes query workloads,
identifies missing indexes, detects unused indexes, and generates
CREATE/DROP INDEX statements with estimated impact. The engine
uses RA's cost model to evaluate hypothetical indexes without
creating them.

## Motivation

Index selection is one of the highest-impact database tuning
decisions and one of the hardest to get right manually. DBAs must
balance:

- Query performance improvement from new indexes
- Write overhead from maintaining indexes
- Storage cost
- Index interaction effects (a new index may make another redundant)

Automated recommendation using the RA optimizer's cost model can
evaluate thousands of hypothetical index configurations and select
the optimal set for a given workload.

## Guide-Level Explanation

### Workload Analysis

```bash
# Analyze recent queries and recommend indexes
ra-cli index recommend --database postgres://localhost/mydb

# Analyze a specific query file
ra-cli index recommend --queries workload.sql

# Limit to specific tables
ra-cli index recommend --tables orders,customers,line_items
```

### Output

```
INDEX RECOMMENDATIONS (3 found):

1. CREATE INDEX idx_orders_customer_date
   ON orders(customer_id, order_date);

   Benefit: 12 queries improved, avg 8.3x speedup
   Cost: 45MB storage, ~5% write overhead
   Net impact: +92% read improvement, -5% write

2. CREATE INDEX idx_line_items_order
   ON line_items(order_id) INCLUDE (quantity, price);

   Benefit: 8 queries improved, avg 15.1x speedup
   Cost: 120MB storage, ~3% write overhead
   Net impact: +88% read improvement, -3% write

3. DROP INDEX idx_orders_legacy;

   Reason: 0 queries use this index in the past 30 days
   Savings: 28MB storage, ~2% write overhead recovered
```

### Hypothetical Index Analysis

```bash
# What-if analysis for a proposed index
ra-cli index evaluate \
  --index "orders(customer_id, order_date)" \
  --queries workload.sql
```

## Reference-Level Explanation

### Architecture

```
Workload (queries + frequencies)
  |
  v
Column Usage Analysis
  |-- Extract predicates, join keys, ORDER BY, GROUP BY
  |-- Track column co-occurrence patterns
  |
  v
Candidate Generation
  |-- Single-column indexes for high-selectivity predicates
  |-- Composite indexes for correlated column access
  |-- Covering indexes for index-only scan opportunities
  |
  v
Cost Evaluation (using RA optimizer)
  |-- For each candidate, estimate plan cost with/without index
  |-- Account for write overhead and storage
  |
  v
Candidate Selection (knapsack-style)
  |-- Maximize total workload improvement
  |-- Subject to storage budget and write overhead limits
  |
  v
Recommendations
```

### Hypothetical Indexes

The engine uses "hypothetical indexes" -- index definitions that
exist only in the cost model, not in the database. The RA optimizer
evaluates plans as if the index existed, using cardinality estimates
to predict scan selectivity.

```rust
pub struct HypotheticalIndex {
    pub table: String,
    pub columns: Vec<IndexColumn>,
    pub include: Vec<String>,
    pub index_type: IndexType,
    pub estimated_size_bytes: u64,
    pub estimated_write_overhead: f64,
}
```

### Workload Model

The workload is represented as a weighted set of queries:

```rust
pub struct Workload {
    pub queries: Vec<WorkloadQuery>,
}

pub struct WorkloadQuery {
    pub sql: String,
    pub frequency: f64,
    pub importance: f64,
}
```

Frequencies can be extracted from `pg_stat_statements` or provided
manually.

### Selection Algorithm

The selection algorithm solves a variant of the index selection
problem (known to be NP-hard) using a greedy approach:

1. Generate candidate indexes
2. Score each candidate by total workload improvement
3. Greedily select the highest-scoring candidate
4. Re-score remaining candidates (accounting for interactions)
5. Repeat until budget is exhausted or no improvement remains

## Drawbacks

- Index recommendation quality depends on accurate statistics
- The workload sample may not be representative of all query
  patterns
- Composite index column ordering requires heuristics
- Cannot account for lock contention during index creation

## Rationale and Alternatives

**Alternative: Use pg_qualstats + HypoPG.** These PostgreSQL
extensions provide similar functionality. However, they don't use
RA's cross-database cost model and cannot recommend indexes for
non-PostgreSQL databases.

**Alternative: Full enumeration.** Evaluate all possible index
combinations. Optimal but exponential in the number of candidate
columns. The greedy approach provides good results in polynomial
time.

## Prior Art

- Microsoft AutoAdmin (DTA) -- the seminal index recommendation tool
- Oracle SQL Access Advisor
- HypoPG -- hypothetical indexes for PostgreSQL
- Dexter -- automatic index recommendations for PostgreSQL
- DB2 Advisor

## Unresolved Questions

- How to handle partial indexes (WHERE clause on the index)?
- Should the engine recommend expression indexes?
- How to account for index maintenance during bulk loads?
- What storage budget should be the default?

## Future Possibilities

- Materialized view recommendation (RFC extends naturally)
- Partition recommendation based on query access patterns
- Online index recommendation that adapts to workload shifts
- Integration with the monitoring system (RFC 0012) for continuous
  index tuning

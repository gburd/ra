# CMU Seminar: MariaDB Query Optimizer

**Source:** https://db.cs.cmu.edu/seminar2025/ (SQL or Death? series)
**Date:** 2025-04-14
**Speaker:** Michael Widenius

## Key Points
- MariaDB optimizer evolved from MySQL with significant extensions
- Multi-range read optimization for index scans
- Condition pushdown to storage engines
- Subquery optimization improvements over MySQL

## Optimization Techniques
- **Multi-Range Read (MRR)**: batch index lookups, sort by rowid, then sequential table access - converts random I/O to sequential
- **Batched Key Access (BKA)**: combine MRR with join processing for nested loop joins
- **Index Condition Pushdown (ICP)**: push WHERE conditions to storage engine during index scan
- **Derived table merge**: merge derived tables (subqueries in FROM) into outer query
- **Semi-join optimizations**: FirstMatch, LooseScan, DuplicateWeedout, Materialization
- **Histogram-based optimization**: JSON histogram format, per-column histograms
- **Engine condition pushdown**: for NDB Cluster, push conditions to data nodes

## Applicable to RA
- Gap: No multi-range read optimization rules
- Gap: No batched key access join strategy
- Gap: No index condition pushdown optimization
- Gap: Limited semi-join optimization rules (FirstMatch, LooseScan, etc.)
- Gap: No derived table merge cost analysis

## References
- MariaDB Knowledge Base: Query Optimizer
- MySQL Optimizer documentation

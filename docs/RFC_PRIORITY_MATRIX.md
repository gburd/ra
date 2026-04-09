# RFC Implementation Priority Matrix

Date: 2026-04-09

## Status Legend
- **Implemented**: Code exists and RFC marked complete
- **Partial**: Some implementation exists (parser, stubs, or partial logic)
- **Not Started**: No meaningful implementation beyond RFC document

---

## Implemented

| RFC | Title | Status | Implementation |
|-----|-------|--------|----------------|
| 0082 | MongoDB Formal Semantics / TOAST / HOT | Implemented | `ra-core/src/document_algebra.rs` (1,880 lines) |

---

## High Priority (P0-P1)

| RFC | Title | Status | Effort | Dependencies | Next Steps |
|-----|-------|--------|--------|--------------|------------|
| 0097 | GROUPING SETS / CUBE / ROLLUP | Partial (parser) | 3-4 weeks | None | Add RelExpr representation, optimization rules, single-pass execution |
| 0098 | LATERAL Subquery Optimization | Partial (executor) | 4-6 weeks | None | Decorrelation rules, cost model for lateral joins |
| 0094 | JSON_TABLE Optimization | Partial (parser) | 4-6 weeks | 0055 | RelExpr mapping, predicate pushdown into JSON paths |
| 0064 | Vector Similarity Search | Partial (rules, hybrid search) | 2-3 weeks | None | Complete HNSW/IVFFlat cost model, pre/post-filter strategy |
| 0059 | Statistics-Based Plan Cache Invalidation | Partial (plan cache exists) | 3-4 weeks | None | Wire differential dataflow to statistics changes |
| 0069 | Execution Feedback Loop | Partial (adaptive calibration) | 4-6 weeks | None | Collect actual vs estimated cardinalities, train correction model |
| 0095 | ASOF Join | Partial (parser AST) | 4-5 weeks | None | Sort-merge ASOF algorithm, cost model, optimizer rules |

---

## Medium Priority (P2)

| RFC | Title | Status | Effort | Dependencies | Next Steps |
|-----|-------|--------|--------|--------------|------------|
| 0096 | PIVOT / UNPIVOT | Partial (parser) | 3-4 weeks | None | RelExpr lowering, optimization rules for single-pass aggregation |
| 0079 | PostgreSQL RUM Index | Partial (rum_index.rs) | 2-3 weeks | 0067 | Complete cost model, index recommendation integration |
| 0067 | Full-Text Search Optimization | Partial (hybrid_search) | 3-4 weeks | None | Ranking deferral, GIN vs GiST selection |
| 0065 | Time-Series Query Optimization | Partial (timeseries.rs, profiles) | 4-6 weeks | 0061 | Chunk pruning, compression-aware cost model |
| 0072 | Adaptive Parallelism | Partial (hardware detection) | 6-8 weeks | 0074 | DOP estimation per operator, work-stealing scheduler |
| 0081 | CitusDB Distributed Query Rules | Partial (citus_optimizer.rs) | 4-5 weeks | 0085 | Co-location detection, shard pruning, distributed agg pushdown |
| 0055 | RDBMS-Specific Type Support | Partial (type system stubs) | 4-6 weeks | None | Type-aware predicate transforms, index recommendations |
| 0070 | Memory-Pressure-Aware Joins | Partial (triggers, plan_switch) | 3-4 weeks | None | Runtime memory monitoring, graceful hash-to-merge fallback |
| 0063 | Spatial Query Optimization | Partial (PostGIS profile) | 4-6 weeks | 0061 | Spatial predicate cost tiers, SRID-aware planning |
| 0101 | Selection Vector Propagation | Partial (vectorized tests) | 3-4 weeks | None | Bitmap/index array through operator pipeline |
| 0099 | Semi-Structured Data Types | Partial (parser support) | 6-8 weeks | 0055 | VARIANT/LIST/STRUCT cost model, nested field statistics |
| 0093 | SQL Property Graph Queries | Partial (parser) | 6-8 weeks | None | MATCH clause lowering, path pattern optimization |
| 0105a | Timeline Enhanced Format | Draft | 2-3 weeks | None | SQL DDL parsing in timelines, parametric functions |

---

## Low Priority (P3)

| RFC | Title | Status | Effort | Dependencies | Next Steps |
|-----|-------|--------|--------|--------------|------------|
| 0053 | Stored Procedure Dialect Support | Not Started | 8-12 weeks | 0055 | PL/pgSQL parser, control flow analysis |
| 0054 | Streaming Plan Adjustments | Not Started | 6-8 weeks | 0059 | Plan fingerprinting, threshold monitoring |
| 0056 | PostgreSQL Type-Specific Optimizations | Not Started | 4-6 weeks | 0055 | JSONB rewrite rules, TOAST cost model |
| 0057 | Cross-Database Type Adaptation | Not Started | 6-8 weeks | 0055 | Storage format detection, per-DB cost overrides |
| 0061 | PostgreSQL Extension-Aware Optimization | Partial (planner_hook) | 4-6 weeks | 0085 | Extension detection API, capability registry |
| 0071 | Workload Classification | Not Started | 3-4 weeks | None | OLTP/OLAP classifier, strategy selection |
| 0073 | Buffer Pool-Aware Planning | Not Started | 3-4 weeks | None | Hot/cold table detection, cache-aware cost model |
| 0074 | Resource-Aware Scheduling | Not Started | 6-8 weeks | 0071 | Resource estimation per query, admission control |
| 0075 | Multi-Objective Cost Model | Not Started | 8-12 weeks | None | Pareto-optimal plan enumeration |
| 0076 | Adaptive Mid-Query Re-Optimization | Not Started | 8-12 weeks | 0069 | Checkpoint insertion, runtime replanning |
| 0077 | NUMA-Aware Execution | Not Started | 6-8 weeks | 0072 | Thread pinning, NUMA-local memory allocation |
| 0080 | DocumentDB RUM BSON Optimization | Partial (documentdb_optimizer.rs) | 3-4 weeks | 0079 | BSON-aware RUM cost model |
| 0083 | XPath/XQuery Optimization | Not Started | 6-8 weeks | 0055 | XPath predicate pushdown, XML index awareness |
| 0084 | Oracle JSON Relational Duality | Not Started | 4-6 weeks | 0055 | Access path selection for duality views |
| 0085 | Platform-Specific Rule Architecture | Not Started | 4-6 weeks | None | Three-tier rule loading, dialect detection |
| 0100 | Time Travel Queries | Partial (table_formats) | 4-6 weeks | None | Versioned scan operators, temporal statistics |
| 0102 | Cross-Database Full-Text Search | Partial (extends 0067) | 4-6 weeks | 0067 | MySQL MATCH, SQL Server CONTAINS support |
| 0103 | Higher-Order Functions | Not Started | 5-6 weeks | 0099 | Lambda parsing, transform/filter optimization |
| 0104 | Delta Lake MERGE Optimization | Partial (table_formats stub) | 6-8 weeks | None | MERGE strategy selection, partition-aware execution |
| 0105b | External Tables / Cloud Storage | Not Started | 6-8 weeks | None | Cloud-aware cost model, file pruning |

---

## Not Started (No Implementation)

- RFC 0053: Stored Procedure Dialect Support
- RFC 0054: Streaming Plan Adjustments
- RFC 0056: PostgreSQL Type-Specific Optimizations
- RFC 0057: Cross-Database Type Adaptation
- RFC 0071: Workload Classification
- RFC 0073: Buffer Pool-Aware Planning
- RFC 0074: Resource-Aware Scheduling
- RFC 0075: Multi-Objective Cost Model
- RFC 0076: Adaptive Mid-Query Re-Optimization
- RFC 0077: NUMA-Aware Execution
- RFC 0083: XPath/XQuery Optimization
- RFC 0084: Oracle JSON Relational Duality
- RFC 0085: Platform-Specific Rule Architecture
- RFC 0103: Higher-Order Functions
- RFC 0105b: External Tables / Cloud Storage

---

## Recommendations

### Top 5 RFCs to tackle next

1. **RFC 0097 (GROUPING SETS)** - Parser support exists, this is the #2 SQL standard
   feature for OLAP. Moderate effort (3-4 weeks) with high user impact. Single-pass
   multi-level aggregation eliminates N-1 table scans.

2. **RFC 0095 (ASOF JOIN)** - Parser AST exists. Time-series joins are a top request
   for financial and IoT workloads. 50-100x speedup over self-join emulation. DuckDB
   and Snowflake both support this natively.

3. **RFC 0064 (Vector Search)** - Already partially implemented with hybrid search
   and vector rules. Completing the HNSW/IVFFlat cost model and pre/post-filter
   strategy is 2-3 weeks of work with immediate AI/ML application value.

4. **RFC 0098 (LATERAL Subquery)** - Executor exists (`lateral_join.rs`). Adding
   decorrelation optimization rules would unlock 10-100x speedups for top-N-per-group
   patterns. SQL:1999 standard, supported by all modern databases.

5. **RFC 0059 (Statistics-Based Cache Invalidation)** - Plan cache infrastructure
   exists. Wiring differential dataflow to statistics changes closes a production
   correctness gap where stale plans cause silent performance degradation.

### Notes

- There is a duplicate RFC 0105 number: one for "Timeline Enhanced Format" and one
  for "External Tables Optimization". This should be renumbered.
- RFCs 0055/0056/0057 form a type system chain. RFC 0055 (base types) should be
  tackled before 0056 (PG-specific) and 0057 (cross-database).
- RFCs 0069/0076 form a feedback chain. RFC 0069 (feedback loop) should precede
  0076 (mid-query re-optimization).
- The modern SQL features (0093-0105) are highest user-facing value per effort
  because they directly enable query patterns users write today.

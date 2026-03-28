# RFC 0100: Time Travel Queries - Completion Report

**Date**: 2026-03-28
**RFC File**: `/home/gburd/ws/ra/docs/rfcs/0100-time-travel-queries.md`
**Status**: Proposed
**Lines**: 789

## Summary

Created comprehensive RFC 0100 for Time Travel query optimization based on research from Snowflake and Databricks feature gap analyses. This RFC enables Ra to optimize temporal queries that access historical table states, essential for audit, compliance, debugging, and A/B testing in cloud data warehouses.

## Key Features Covered

### 1. Syntax Support

**Snowflake**:
- `AT(TIMESTAMP => '...')` - Query as of specific timestamp
- `AT(OFFSET => -N)` - Query N seconds in the past
- `BEFORE(STATEMENT => '...')` - Query before specific statement
- Retention: 1-90 days depending on edition

**Delta Lake**:
- `VERSION AS OF N` - Query specific transaction version
- `TIMESTAMP AS OF '...'` - Query as of timestamp
- Shorthand: `table@v42` or `table@20240101000000000`
- Configurable retention policies

**Cross-Database**:
- MariaDB: `FOR SYSTEM_TIME AS OF`
- SQL Server: `FOR SYSTEM_TIME` temporal tables

### 2. Implementation Approaches

**MVCC-Based** (PostgreSQL, Oracle):
- Multiple row versions with validity periods
- Fast access to any version
- Storage overhead for version chains

**Copy-on-Write** (Delta Lake, Iceberg):
- Immutable files with transaction log
- Simple version reconstruction
- File-level granularity

**Delta Files** (Incremental):
- Base snapshot + incremental changes
- Storage efficient for small changes
- Delta chain traversal for reconstruction

### 3. Core Technical Design

**RelExpr Extension**:
```rust
pub struct Scan {
    pub time_travel: Option<TimeTravelClause>,
}

pub enum TimeTravelClause {
    AtTimestamp(DateTime<Utc>),
    AtOffset(i64),
    BeforeStatement(String),
    AtVersion(u64),
    SystemTimeAsOf(DateTime<Utc>),
}
```

**Cost Model**:
- Recent history (< 1 day): 1.02x - 2.0x overhead
- Distant past (30 days): 5.0x - 74x overhead (depends on format)
- Delta queries: 5-100x speedup with optimization

**Version Metadata Cache**:
- LRU cache with 24-hour TTL
- Immutable historical metadata (no invalidation needed)
- Target: >80% cache hit rate

**Versioned Statistics**:
- Per-version, daily, or logarithmic granularity
- Interpolation for missing versions
- Accurate cardinality estimation

### 4. Optimization Opportunities

**Temporal Partition Pruning**:
- Combine temporal + partition filters
- Filter to partitions active at query timestamp
- 10-100x reduction in scanned data

**Delta Query Optimization**:
- Optimize EXCEPT queries between versions
- Scan only changed partitions
- 10-100x speedup for small deltas

**Version Metadata Caching**:
- Avoid repeated metadata lookups
- 5-10ms → 0.1ms per query
- Critical for dashboard workloads

**Temporal Predicate Pushdown**:
- Push filters into temporal scans
- Reduce rows before version lookup
- Standard optimization extended to temporal dimension

### 5. Optimization Rules

1. **Temporal Predicate Pushdown**: Push filters into temporal scans
2. **Temporal Partition Pruning**: Eliminate partitions by time + filters
3. **Delta Query Optimization**: Convert EXCEPT to incremental delta scan
4. **Version Metadata Caching**: Reuse cached metadata across queries

## Performance Analysis

### Baseline Comparison

**Current Data Query**:
- Table: 100M rows, 1GB compressed
- Query: 10% selectivity
- Cost: 100, Time: 2s

**Recent History (1 day ago, MVCC)**:
- Cost: 102 (2% overhead)
- Time: 2.04s

**Recent History (1 day ago, CoW)**:
- Cost: 200 (100% overhead)
- Time: 4s

**Distant Past (30 days ago)**:
- Cost: 500 (400% overhead)
- Time: 10s

**Delta Query (0.1% change rate)**:
- Without optimization: Cost 400, Time 8s
- With optimization: Cost 50, Time 1s
- **Speedup: 8x**

**Temporal Partition Pruning**:
- Without: 36,500 version checks, Cost 10,000
- With: 10 version checks, Cost 1,000
- **Speedup: 10x**

## Implementation Plan

**Phase 1: Core Infrastructure (6-8 weeks)**
- Add TimeTravelClause enum and extend RelExpr::Scan
- Implement Snowflake AT/BEFORE parser support
- Implement Delta Lake VERSION AS OF parser support
- Add temporal dimension to catalog metadata
- Implement temporal scan executor
- Add temporal scan cost model

**Phase 2: Optimization (8-10 weeks)**
- Version metadata cache with LRU + TTL
- Temporal partition pruning
- Versioned statistics with interpolation
- Optimization rules (pushdown, delta queries)

**Phase 3: Cross-Database Support (4-6 weeks)**
- MariaDB FOR SYSTEM_TIME syntax
- SQL Server temporal table support
- Storage format detection (MVCC vs CoW vs Delta)
- Format-specific cost models
- Dialect translation

**Phase 4: Advanced Features (6-8 weeks)**
- Time range indexes recommendation
- Smart retention policy recommendations
- Cross-version query batching
- Temporal join optimization

**Total Estimated Effort**: 24-32 weeks

## Expected Impact

**High Priority**: Essential for cloud warehouse compliance and debugging

**Performance**:
- 2-5x overhead for recent history (acceptable)
- 10-20x speedup for delta queries with optimization
- 10x speedup from temporal partition pruning

**Use Cases Enabled**:
- Financial audit trails and compliance reporting
- GDPR verification and data lineage
- Debugging incorrect data introduction points
- A/B testing by comparing query results across versions
- Incremental ETL via delta computation
- Data recovery from accidental deletions

**Coverage**: Supports 80%+ of Time Travel patterns in Snowflake and Delta Lake

## Design Decisions

**Why extend Scan instead of new operator?**
- Temporal scans have identical semantics to regular scans
- Existing scan optimization rules apply with minor cost adjustments
- Avoids duplicating scan-related rules
- Matches database implementations

**Why cache version metadata instead of query results?**
- Metadata smaller than query results
- Applies to multiple queries with different filters
- Metadata is immutable (no invalidation complexity)
- Complements existing plan cache

**Why support multiple syntaxes?**
- Users write queries in native database syntax
- Ra's polyglot backend already handles syntax differences
- Optimization opportunities differ by implementation
- Translation between version and timestamp semantics is complex

## Testing Strategy

**Unit Tests**:
- Parse all temporal clause syntaxes
- Version metadata cache operations
- Statistics interpolation accuracy
- Temporal partition pruning logic
- Delta query detection

**Integration Tests**:
- Temporal scans on MVCC storage
- Temporal scans on CoW storage
- Predicate pushdown optimization
- Delta query optimization
- Cross-database dialect translation

**Performance Tests**:
- Temporal scan overhead vs baseline (<5x target)
- Version metadata cache hit rate (>80% target)
- Delta query speedup (>5x target)
- Temporal partition pruning speedup (>10x target)

**Correctness Tests**:
- Verify exact historical state reconstruction
- Verify delta query matches EXCEPT semantics
- Verify partition pruning completeness
- Verify cache consistency

## Success Criteria

1. **Correctness**: 100% identical results to database-native execution
2. **Performance**: <5x overhead for recent history, 5-20x for distant past
3. **Optimization**: >5x speedup for delta queries with <1% change rate
4. **Cache**: >80% version metadata cache hit rate
5. **Coverage**: 80%+ of Snowflake and Delta Lake patterns

## References

- `/home/gburd/ws/ra/SNOWFLAKE_FEATURES_GAP_ANALYSIS.md` - Section 3: Time Travel
- `/home/gburd/ws/ra/DATABRICKS_SPARK_FEATURES_ANALYSIS.md` - Section 1.2: Time Travel
- Snowflake documentation: Time Travel with 1-90 day retention
- Delta Lake documentation: VERSION AS OF and TIMESTAMP AS OF
- SQL:2011 Temporal Data Standard

## RFC Structure

The RFC follows the standard Ra template with these sections:

1. **Summary**: One-paragraph overview of Time Travel support
2. **Motivation**: Why Time Travel is essential (audit, debugging, temporal analysis)
3. **Guide-level explanation**: Syntax examples and optimization scenarios
4. **Reference-level explanation**:
   - Temporal dimension in RelExpr
   - Three implementation approaches (MVCC, CoW, Delta Files)
   - Cost model with overhead multipliers
   - Temporal partition pruning algorithm
   - Version metadata caching strategy
   - Versioned statistics with interpolation
   - Four optimization rules
5. **Drawbacks**: Metadata overhead, retention complexity, interpolation accuracy
6. **Rationale and alternatives**: Design decision justifications
7. **Prior art**: Snowflake, Delta Lake, PostgreSQL, SQL Server, MariaDB
8. **Unresolved questions**: Four open questions for community feedback
9. **Future possibilities**: Temporal indexes, retention recommendations, cross-version optimization
10. **Implementation plan**: Four-phase plan totaling 24-32 weeks
11. **Performance analysis**: Detailed cost examples and speedup scenarios
12. **Testing strategy**: Unit, integration, performance, correctness tests
13. **Success criteria**: Five measurable goals

## Completion Checklist

- [x] RFC structure follows template
- [x] Covers Snowflake AT/BEFORE/OFFSET syntax
- [x] Covers Delta Lake VERSION AS OF / TIMESTAMP AS OF
- [x] Includes MariaDB and SQL Server variants
- [x] Three implementation approaches documented
- [x] Cost model with overhead multipliers
- [x] Temporal partition pruning algorithm
- [x] Version metadata caching strategy
- [x] Versioned statistics with interpolation
- [x] Four optimization rules defined
- [x] Performance analysis with concrete examples
- [x] Four-phase implementation plan (24-32 weeks)
- [x] Testing strategy covering all test types
- [x] Success criteria measurable
- [x] References to gap analysis documents
- [x] 789 lines of comprehensive technical content

## Next Steps

1. Solicit feedback from Ra maintainers on:
   - Retention policy handling approach
   - Version count estimation strategy
   - Temporal queries in materialized views
   - Storage format detection mechanism

2. Begin Phase 1 implementation:
   - Add TimeTravelClause enum
   - Extend RelExpr::Scan
   - Implement parser support

3. Create tracking issue in Ra repository

4. Update Snowflake and Databricks gap analyses to mark Time Travel as "RFC proposed"

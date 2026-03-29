# RFC 0100: Time Travel Queries - Summary

**Created**: 2026-03-28
**RFC File**: `/home/gburd/ws/ra/docs/rfcs/0100-time-travel-queries.md`
**Branch**: `rfc-0100-time-travel`
**Commit**: `2672b892`
**Status**: Proposed
**Lines**: 789

## Overview

Created comprehensive RFC 0100 for Time Travel query optimization, enabling Ra to optimize temporal queries that access historical table states. This feature is essential for audit trails, compliance reporting, debugging, and A/B testing in cloud data warehouses.

## Key Capabilities

### Syntax Support

**Snowflake (1-90 day retention)**:
```sql
-- AT TIMESTAMP
SELECT * FROM orders AT(TIMESTAMP => '2024-01-01 12:00:00') WHERE region = 'WEST';

-- AT OFFSET
SELECT * FROM inventory AT(OFFSET => -300) WHERE stock < 10;

-- BEFORE STATEMENT
SELECT * FROM products BEFORE(STATEMENT => 'query_id') WHERE category = 'electronics';
```

**Delta Lake (configurable retention)**:
```sql
-- VERSION AS OF
SELECT * FROM sales VERSION AS OF 42 WHERE amount > 1000;

-- TIMESTAMP AS OF
SELECT * FROM customers TIMESTAMP AS OF '2024-01-01' WHERE status = 'active';

-- Shorthand
SELECT * FROM sales@v42;
SELECT * FROM sales@20240101000000000;
```

**Cross-Database**:
- MariaDB: `FOR SYSTEM_TIME AS OF TIMESTAMP '...'`
- SQL Server: `FOR SYSTEM_TIME AS OF '...'`

## Technical Design

### Core Extension

```rust
pub enum TimeTravelClause {
    AtTimestamp(DateTime<Utc>),      // Snowflake, Delta Lake
    AtOffset(i64),                   // Snowflake
    BeforeStatement(String),         // Snowflake
    AtVersion(u64),                  // Delta Lake
    SystemTimeAsOf(DateTime<Utc>),   // MariaDB, SQL Server
}

pub struct Scan {
    pub time_travel: Option<TimeTravelClause>,  // NEW
    // ... existing fields
}
```

### Implementation Approaches

1. **MVCC-Based** (PostgreSQL, Oracle)
   - Multiple row versions with validity periods
   - Fast version access, higher storage overhead

2. **Copy-on-Write** (Delta Lake, Iceberg)
   - Immutable files with transaction log
   - Simple reconstruction, file-level granularity

3. **Delta Files** (Incremental)
   - Base snapshot + change deltas
   - Storage efficient, slower for distant past

### Cost Model

| Scenario | Base | Overhead | Total | Notes |
|----------|------|----------|-------|-------|
| Current data | 100 | 1.0x | 100 | Baseline |
| 1 hour ago (MVCC) | 100 | 1.02x | 102 | Minimal version chain |
| 1 day ago (CoW) | 100 | 2.0x | 200 | Metadata + cold cache |
| 30 days ago (CoW) | 100 | 5.0x | 500 | Cold metadata + storage |
| 30 days ago (Delta) | 100 | 74x | 7400 | Many delta traversals |

### Optimization Strategies

**1. Temporal Partition Pruning**
- Filter to partitions active at query timestamp
- Combine with query predicates
- **Speedup: 10-100x**

**2. Delta Query Optimization**
- Convert `EXCEPT` between versions to incremental delta scan
- Scan only changed partitions
- **Speedup: 8-100x for <1% change rate**

**3. Version Metadata Caching**
- LRU cache with 24-hour TTL
- Immutable historical metadata
- **5-10ms → 0.1ms per query**

**4. Versioned Statistics**
- Per-version, daily, or logarithmic granularity
- Interpolation for missing versions
- Accurate cardinality estimation

## Performance Examples

### Delta Query (0.1% change rate)

**Without optimization**:
- Cost: 100 × 2 × 2.0 = 400
- Time: 8 seconds
- Scans: Full table twice

**With delta optimization**:
- Cost: 100K × 0.5 = 50
- Time: 1 second
- Scans: Changed rows only
- **Speedup: 8x**

### Temporal Partition Pruning

**Without pruning**:
- Partitions: 100 × 365 versions = 36,500
- Cost: 10,000
- Time: 20 seconds

**With pruning**:
- Partitions: 10 (active at timestamp)
- Cost: 1,000
- Time: 2 seconds
- **Speedup: 10x**

## Implementation Plan

### Phase 1: Core Infrastructure (6-8 weeks)
- Add TimeTravelClause enum
- Extend RelExpr::Scan
- Snowflake AT/BEFORE parser
- Delta Lake VERSION AS OF parser
- Temporal catalog metadata
- Temporal scan executor
- Cost model implementation

### Phase 2: Optimization (8-10 weeks)
- Version metadata cache (LRU + TTL)
- Temporal partition pruning
- Versioned statistics with interpolation
- Optimization rules:
  - Temporal predicate pushdown
  - Delta query optimization
  - Temporal + partition coordination
  - Metadata caching

### Phase 3: Cross-Database (4-6 weeks)
- MariaDB FOR SYSTEM_TIME syntax
- SQL Server temporal tables
- Storage format detection
- Format-specific cost models
- Dialect translation

### Phase 4: Advanced Features (6-8 weeks)
- Time range indexes recommendation
- Smart retention policy recommendations
- Cross-version query batching
- Temporal join optimization

**Total Effort**: 24-32 weeks

## Expected Impact

### Priority: HIGH
Essential for cloud warehouse compliance and debugging

### Use Cases Enabled
- Financial audit trails and compliance reporting
- GDPR right-to-be-forgotten verification
- SOX compliance auditing
- Identifying when incorrect data was introduced
- Restoring accidentally deleted rows
- A/B testing by comparing query results
- Time-series analysis of changing data
- Incremental ETL via delta computation

### Performance Targets
- Recent history (<1 day): <5x overhead
- Distant past (30 days): 5-20x overhead
- Delta queries (<1% change): >5x speedup
- Temporal partition pruning: >10x speedup
- Version metadata cache hit rate: >80%

### Coverage
Supports 80%+ of Time Travel patterns in Snowflake and Delta Lake

## Design Highlights

### Why Extend Scan vs New Operator?
- Temporal scans have identical semantics to regular scans
- Existing optimization rules apply with minor cost adjustments
- Avoids duplicating scan-related rules
- Matches database implementations

### Why Cache Metadata vs Query Results?
- Metadata is smaller than query results
- Applies to multiple queries with different filters
- Immutable (no invalidation complexity)
- Complements existing plan cache

### Why Support Multiple Syntaxes?
- Users write queries in native database syntax
- Ra's polyglot backend already handles syntax differences
- Optimization opportunities differ by implementation
- Version vs timestamp semantics differ fundamentally

## Success Criteria

1. **Correctness**: 100% identical results to database-native execution
2. **Performance**: <5x overhead for recent history
3. **Optimization**: >5x speedup for delta queries (<1% change rate)
4. **Cache**: >80% version metadata cache hit rate
5. **Coverage**: 80%+ of Snowflake and Delta Lake patterns

## Testing Strategy

### Unit Tests
- Parse all temporal clause syntaxes
- Version metadata cache operations
- Statistics interpolation accuracy
- Temporal partition pruning logic
- Delta query detection

### Integration Tests
- Temporal scans on MVCC storage
- Temporal scans on CoW storage
- Predicate pushdown optimization
- Delta query optimization
- Cross-database dialect translation

### Performance Tests
- Temporal scan overhead (<5x target)
- Cache hit rate (>80% target)
- Delta query speedup (>5x target)
- Partition pruning speedup (>10x target)

### Correctness Tests
- Exact historical state reconstruction
- Delta query EXCEPT semantics
- Partition pruning completeness
- Cache consistency

## References

- **Snowflake Gap Analysis**: `/home/gburd/ws/ra/SNOWFLAKE_FEATURES_GAP_ANALYSIS.md` (Section 3)
- **Databricks Analysis**: `/home/gburd/ws/ra/DATABRICKS_SPARK_FEATURES_ANALYSIS.md` (Section 1.2)
- **Related RFCs**:
  - RFC 0061: PostgreSQL Extension-Aware Optimization
  - RFC 0026: Adaptive Cost Calibration
  - RFC 0025: Physical Property Tracking

## RFC Structure (789 lines)

1. **Summary**: One-paragraph overview
2. **Motivation**: Audit, compliance, debugging use cases
3. **Guide-level explanation**: Syntax examples and optimization scenarios
4. **Reference-level explanation**:
   - Temporal dimension in RelExpr (Rust code)
   - Three implementation approaches
   - Cost model with overhead multipliers
   - Temporal partition pruning algorithm
   - Version metadata caching strategy
   - Versioned statistics with interpolation
   - Four optimization rules
5. **Drawbacks**: Metadata overhead, retention complexity, interpolation accuracy
6. **Rationale and alternatives**: Three key design decisions justified
7. **Prior art**: Snowflake, Delta Lake, PostgreSQL, SQL Server, MariaDB
8. **Unresolved questions**: Four open questions for feedback
9. **Future possibilities**: Five advanced features
10. **Implementation plan**: Four phases, 24-32 weeks
11. **Performance analysis**: Six detailed scenarios with costs
12. **Testing strategy**: Unit, integration, performance, correctness
13. **Success criteria**: Five measurable goals

## Next Steps

1. **Solicit feedback** from Ra maintainers on:
   - Retention policy handling approach
   - Version count estimation strategy
   - Temporal queries in materialized views
   - Storage format detection mechanism

2. **Create tracking issue** in Ra repository

3. **Begin Phase 1 implementation**:
   - Add TimeTravelClause enum to ra-core
   - Extend RelExpr::Scan in ra-core
   - Implement Snowflake parser in ra-parser
   - Implement Delta Lake parser in ra-parser

4. **Update gap analyses** to mark Time Travel as "RFC proposed"

## Files Created

- `/home/gburd/ws/ra/docs/rfcs/0100-time-travel-queries.md` (789 lines)
- `/home/gburd/ws/ra/RFC_0100_COMPLETION.md` (detailed completion report)
- `/home/gburd/ws/ra/RFC_0100_SUMMARY.md` (this file)

## Git Information

```
Branch: rfc-0100-time-travel
Commit: 2672b892
Files: 2 changed, 1103 insertions(+)
Status: Ready for review
```

## Completion Checklist

- [x] RFC structure follows template
- [x] Comprehensive syntax coverage (Snowflake, Delta Lake, MariaDB, SQL Server)
- [x] Three implementation approaches documented with tradeoffs
- [x] Cost model with concrete overhead multipliers
- [x] Four optimization strategies with speedup estimates
- [x] Performance analysis with six detailed scenarios
- [x] Four-phase implementation plan (24-32 weeks)
- [x] Comprehensive testing strategy
- [x] Five measurable success criteria
- [x] References to gap analysis documents
- [x] Rust code examples for core extensions
- [x] SQL syntax examples for all supported databases
- [x] 789 lines of technical content
- [x] Committed to branch rfc-0100-time-travel
- [x] Completion and summary documents created

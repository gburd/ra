# RFC 0100: Time Travel Queries - Quick Reference

**File**: `/home/gburd/ws/ra/docs/rfcs/0100-time-travel-queries.md`
**Branch**: `rfc-0100-time-travel`
**Status**: Proposed
**Effort**: 24-32 weeks
**Impact**: HIGH - Essential for compliance

## Syntax Cheatsheet

### Snowflake
```sql
AT(TIMESTAMP => '2024-01-01 12:00:00')  -- Specific timestamp
AT(OFFSET => -300)                      -- 300 seconds ago
BEFORE(STATEMENT => 'query_id')         -- Before statement
```

### Delta Lake
```sql
VERSION AS OF 42                        -- Version number
TIMESTAMP AS OF '2024-01-01'           -- Timestamp
table@v42                               -- Shorthand version
table@20240101000000000                 -- Shorthand timestamp
```

### MariaDB / SQL Server
```sql
FOR SYSTEM_TIME AS OF '2024-01-01'     -- Temporal table
```

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Recent history overhead | <5x | Within 1 day |
| Distant past overhead | 5-20x | 30+ days |
| Delta query speedup | >5x | <1% change rate |
| Partition pruning speedup | >10x | Combined filters |
| Cache hit rate | >80% | Version metadata |

## Key Optimizations

1. **Temporal Partition Pruning**: 10-100x speedup
2. **Delta Query Optimization**: 8-100x speedup
3. **Version Metadata Caching**: 5-10ms → 0.1ms
4. **Versioned Statistics**: Accurate cardinality

## Implementation Phases

1. **Core** (6-8 weeks): TimeTravelClause, parser, executor
2. **Optimization** (8-10 weeks): Cache, pruning, statistics
3. **Cross-DB** (4-6 weeks): MariaDB, SQL Server, formats
4. **Advanced** (6-8 weeks): Indexes, retention, batching

## Use Cases

- Financial audit trails
- GDPR verification
- Debugging data issues
- A/B testing
- Incremental ETL
- Data recovery

## Success Criteria

✓ 100% correctness vs native execution
✓ <5x overhead for recent history
✓ >5x speedup for delta queries
✓ >80% cache hit rate
✓ 80%+ pattern coverage

## Files

- RFC: `docs/rfcs/0100-time-travel-queries.md` (789 lines)
- Completion: `RFC_0100_COMPLETION.md`
- Summary: `RFC_0100_SUMMARY.md`
- Quick Ref: `RFC_0100_QUICK_REFERENCE.md`

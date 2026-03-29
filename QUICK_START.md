# RFC 0094: JSON_TABLE - Quick Start Guide

## Viewing the RFC

### Main RFC Document
```bash
# Full RFC (1,098 lines)
less docs/rfcs/0094-json-table-optimization.md

# Or open in your editor
$EDITOR docs/rfcs/0094-json-table-optimization.md
```

### Completion Summary
```bash
# Executive summary (397 lines)
less RFC_0094_COMPLETION_SUMMARY.md
```

## Key Sections to Review

### For Decision Makers (30 minutes read)
1. **Summary** (lines 1-12): One-paragraph overview
2. **Motivation** (lines 14-121): Why JSON_TABLE matters
3. **Database Support** (lines 50-58): All 8 databases support it
4. **Performance Impact** (lines 60-66): 3-10x speedup expected
5. **Implementation Plan** (lines 614-727): 17-21 weeks in 6 phases
6. **Success Criteria** (RFC_0094_COMPLETION_SUMMARY.md lines 359-367)

### For Developers (2 hours read)
1. **Grammar Extensions** (lines 296-324): Parser changes needed
2. **Relational Algebra** (lines 326-391): RelExpr::JsonTable design
3. **Optimization Rules** (lines 393-554): 6 rules with algorithms
4. **Cost Model** (lines 556-612): Formulas and heuristics
5. **Testing Strategy** (lines 729-803): Test coverage plan

### For Researchers (full read)
- Read entire RFC (1,098 lines)
- Review Prior Art section (lines 938-1009)
- Check References (lines 1069-1098)
- Compare with related RFCs (0083, 0084, 0093)

## Quick Stats

| Metric | Value |
|--------|-------|
| **RFC Length** | 1,098 lines (31 KB) |
| **Implementation Effort** | 17-21 weeks (4-5 months) |
| **Expected Speedup** | 3-10x (up to 50x with indexes) |
| **Database Support** | 8/8 major databases |
| **Test Cases** | 220+ tests planned |
| **Optimization Rules** | 6 major rules |

## Example Queries

### Simple Array Unnesting
```sql
SELECT jt.item_id, jt.quantity
FROM orders o,
     JSON_TABLE(o.items, '$[*]' COLUMNS(
       item_id INT PATH '$.id',
       quantity INT PATH '$.qty'
     )) AS jt;
```

### Nested JSON Structures
```sql
SELECT jt.order_id, jt.item_id, jt.spec_name
FROM orders o,
     JSON_TABLE(o.order_data, '$' COLUMNS(
       order_id INT PATH '$.id',
       NESTED PATH '$.items[*]' COLUMNS(
         item_id INT PATH '$.id',
         NESTED PATH '$.specs[*]' COLUMNS(
           spec_name VARCHAR(50) PATH '$.name'
         )
       )
     )) AS jt;
```

### With Filter Pushdown (5-10x faster)
```sql
-- Optimizer pushes price > 100 into JSONPath
SELECT jt.item_id
FROM orders o,
     JSON_TABLE(o.items, '$[*]' COLUMNS(
       item_id INT PATH '$.id',
       price DECIMAL PATH '$.price'
     )) AS jt
WHERE jt.price > 100;

-- Becomes: JSON_TABLE(o.items, '$[?(@.price > 100)]' ...)
```

## Implementation Phases

| Phase | Weeks | Focus | Deliverables |
|-------|-------|-------|--------------|
| 1. Parser | 3-4 | Grammar extensions | sqlparser-rs changes, 50+ tests |
| 2. Translation | 2-3 | RelExpr design | RelExpr::JsonTable, type checking |
| 3. Optimization | 4-5 | Rewrite rules | 6 rules, integration tests |
| 4. Cost Model | 2-3 | Statistics & costs | Cost functions, cardinality estimates |
| 5. Dialect | 2 | Cross-database | OPENJSON, FLATTEN translations |
| 6. Testing | 2-3 | Benchmarks | 220+ tests, performance report |

**Total: 17-21 weeks**

## Optimization Strategies

1. **JSONPath Predicate Pushdown** → 5-10x speedup
2. **JSON Index Scan** → 10-50x speedup (with indexes)
3. **Parallel Array Unnesting** → 8-15x speedup (large arrays)
4. **Column Pruning** → 2-4x speedup
5. **Late Materialization** → 2-4x speedup
6. **Single-Pass Nested Extraction** → 4-8x speedup

## Git Information

**Branch:** `rfc-0094-json-table`
**Worktree:** `/home/gburd/ws/ra/.claude/worktrees/rfc-0094-json-table`
**Commits:**
- `f6e2c18a` - Main RFC document
- `53d0b459` - Completion summary

## Next Steps

1. **Review RFC** with Ra development team
2. **Gather feedback** and address questions
3. **Approve RFC** with project maintainers
4. **Create tracking issue** in GitHub
5. **Begin Phase 1** (parser support)

## Questions?

Key sections to clarify common questions:

- **Why JSON_TABLE?** → See "Motivation" (lines 14-121)
- **How does it work?** → See "Guide-level explanation" (lines 123-294)
- **What's the performance gain?** → See "Performance Benchmarks" (lines 805-866)
- **How long to implement?** → See "Implementation Plan" (lines 614-727)
- **What are the risks?** → See "Drawbacks" (lines 868-899)

## Research Sources

This RFC is based on:
- **SQL_STANDARDS_GAP_ANALYSIS.md** - Identified JSON_TABLE as #1 priority
- **MYSQL_MARIADB_UNSUPPORTED_FEATURES.md** - MySQL/MariaDB specifics
- **RFC 0083** - XPath/XQuery optimization (similar patterns)
- **RFC 0084** - Oracle JSON Duality Views
- **RFC 0093** - SQL Property Graph Queries

---

**RFC Status:** ✅ Complete and Ready for Review
**Last Updated:** 2026-03-28

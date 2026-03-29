# RFC 0094: JSON_TABLE Optimization - Completion Summary

**Date:** 2026-03-28
**Author:** Ra Research Team
**Status:** ✅ Complete and Ready for Review

---

## Overview

RFC 0094 has been created as a comprehensive design document for implementing SQL:2016 JSON_TABLE support in the Ra query optimizer. This feature is identified as the #1 priority missing SQL standard feature based on research from SQL_STANDARDS_GAP_ANALYSIS.md and MYSQL_MARIADB_UNSUPPORTED_FEATURES.md.

**Document Location:** `/home/gburd/ws/ra/docs/rfcs/0094-json-table-optimization.md`
**Branch:** `rfc-0094-json-table`
**Commit:** `f6e2c18a`

---

## Key Statistics

- **RFC Length:** 1,098 lines
- **Estimated Implementation Effort:** 17-21 weeks (4-5 months)
- **Expected Performance Impact:** 3-10x speedup (up to 50x with indexes)
- **Database Coverage:** 8/8 major databases support JSON_TABLE
- **Priority:** HIGH - Critical for modern JSON analytics workloads

---

## What is JSON_TABLE?

JSON_TABLE is a SQL:2016 standard feature that converts JSON data into relational table format. It enables:

1. **Declarative JSON unnesting** - Convert JSON arrays to rows with typed columns
2. **Nested structure support** - Handle hierarchical JSON with NESTED PATH
3. **Type safety** - Explicit type conversion with error handling
4. **Standard syntax** - Works across PostgreSQL, MySQL, Oracle, SQL Server, Snowflake

### Example

**Before (verbose):**
```sql
-- PostgreSQL: Lateral join with jsonb_array_elements
SELECT o.order_id, item.value->>'id' AS item_id
FROM orders o, LATERAL jsonb_array_elements(o.items) AS item;
```

**After (JSON_TABLE):**
```sql
SELECT o.order_id, jt.item_id, jt.quantity
FROM orders o,
     JSON_TABLE(o.items, '$[*]' COLUMNS(
       item_id INT PATH '$.id',
       quantity INT PATH '$.qty'
     )) AS jt;
```

---

## Database Support

| Database | Version | Support | JSON Index Type | Notes |
|----------|---------|---------|-----------------|-------|
| **PostgreSQL** | 17+ | ✅ | GIN (JSONB) | Standard SQL:2016 |
| **MySQL** | 8.0+ | ✅ | Multi-valued indexes | Binary JSON |
| **Oracle** | 12c+ | ✅ | JSON search indexes | First implementer |
| **SQL Server** | 2016+ | ✅ | Computed column indexes | Uses OPENJSON |
| **MariaDB** | 10.6+ | ✅ | Expression indexes | MySQL compatible |
| **Snowflake** | All | ✅ | VARIANT type | Columnar JSON |
| **Databricks** | All | ✅ | Delta column stats | Parquet-based |
| **MonetDB** | - | ✅ | - | Research database |

**Coverage:** 100% of major databases support JSON_TABLE or equivalent

---

## Optimization Strategies

The RFC proposes 6 major optimization rules:

### 1. JSONPath Predicate Pushdown (5-10x speedup)
Push WHERE clause filters into the JSONPath expression:
```sql
-- Before: Filter after unnesting
SELECT * FROM JSON_TABLE(...) WHERE price > 100;

-- After: Filter during JSON parsing
SELECT * FROM JSON_TABLE('$[?(@.price > 100)]' ...);
```

### 2. JSON Index Scan (10-50x speedup)
Use database-specific JSON indexes:
- PostgreSQL: GIN indexes on JSONB columns
- MySQL: Multi-valued indexes on JSON arrays
- Oracle: JSON search indexes
- SQL Server: Computed column functional indexes

### 3. Parallel Array Unnesting (8-15x speedup)
Partition large JSON arrays across multiple workers:
- Activate for arrays with 1000+ elements
- Near-linear scaling with available CPU cores
- Example: 10,000 element array → 4 workers → 13x speedup

### 4. Column Pruning (2-4x speedup)
Extract only columns used in query:
```sql
-- Only extract item_id, skip unused name/price/description
SELECT item_id FROM JSON_TABLE(...) AS jt;
```

### 5. Late Materialization (2-4x speedup)
Defer JSON parsing until after filtering:
```sql
-- Parse JSON only for rows where order_id = 123
SELECT * FROM orders WHERE order_id = 123
  CROSS JOIN JSON_TABLE(orders.items, ...);
```

### 6. Single-Pass Nested Extraction (4-8x speedup)
Optimize nested JSON structures to avoid redundant parsing:
```sql
-- Parse entire document once for nested COLUMNS
NESTED PATH '$.items[*]' COLUMNS(
  item_id PATH '$.id',
  NESTED PATH '$.specs[*]' COLUMNS(...)
)
```

---

## Performance Benchmarks

Expected performance improvements for typical JSON queries:

| Query Pattern | Baseline | Optimized | Speedup | Optimization |
|---------------|----------|-----------|---------|--------------|
| Simple array unnesting | 150ms | 45ms | **3.3x** | Column pruning, parallel |
| Filtered array elements | 180ms | 25ms | **7.2x** | Predicate pushdown |
| Nested JSON (3 levels) | 450ms | 120ms | **3.8x** | Single-pass extraction |
| Large arrays (10K elements) | 8000ms | 600ms | **13.3x** | Parallel unnesting |
| With JSON index | 45000ms | 800ms | **56x** | Index scan |

**Overall target:** 3-10x speedup for typical queries, up to 50x with indexes

---

## RFC Structure

The RFC includes the following sections:

### 1. Summary and Motivation (Lines 1-121)
- Problem statement and current limitations
- JSON_TABLE benefits and use cases
- Database support matrix
- Expected performance impact

### 2. Guide-Level Explanation (Lines 123-294)
- Basic JSON_TABLE syntax and examples
- Column type specifications
- Nested JSON structures with NESTED PATH
- Error handling (ON ERROR, ON EMPTY)
- Ordinal columns (FOR ORDINALITY)

### 3. Reference-Level Explanation (Lines 296-612)
- Grammar extensions for parser
- Relational algebra representation (RelExpr::JsonTable)
- 6 detailed optimization rules with algorithms
- Cost model with formulas and heuristics
- Cross-database translation strategies

### 4. Implementation Plan (Lines 614-727)
- Phase 1: Parser support (3-4 weeks)
- Phase 2: Relational algebra translation (2-3 weeks)
- Phase 3: Optimization rules (4-5 weeks)
- Phase 4: Cost model (2-3 weeks)
- Phase 5: Dialect translation (2 weeks)
- Phase 6: Testing and benchmarking (2-3 weeks)

### 5. Testing Strategy (Lines 729-803)
- Unit tests (120+ test cases)
- Integration tests (100+ test cases)
- Performance benchmarks (5 benchmark queries)
- Regression tests

### 6. Performance Benchmarks (Lines 805-866)
- 5 benchmark queries with expected results
- Scalability tests (small, medium, large JSON)
- Index usage tests

### 7. Analysis Sections (Lines 868-1009)
- Drawbacks and limitations
- Rationale and alternatives
- Prior art (Oracle, MySQL, PostgreSQL, SQL Server, Snowflake)
- Unresolved questions

### 8. Future Possibilities (Lines 1011-1067)
- JSON_TABLE extensions (PASSING clause, Schema validation)
- JSON optimization framework
- Cross-feature integration (window functions, temporal queries)

### 9. References (Lines 1069-1098)
- SQL standards documentation
- Database vendor documentation
- Research papers
- Related RFCs

---

## Implementation Phases

### Phase 1: Parser Support (3-4 weeks)
**Deliverables:**
- Extended sqlparser-rs with JSON_TABLE grammar
- AST nodes for all JSON_TABLE constructs
- 50+ parser tests

### Phase 2: Relational Algebra Translation (2-3 weeks)
**Deliverables:**
- RelExpr::JsonTable variant
- SQL AST to RelExpr translation
- Type checking and validation

### Phase 3: Optimization Rules (4-5 weeks)
**Deliverables:**
- 6 optimization rules in egg rewrite system
- Integration tests for each rule
- Rule ordering and applicability logic

### Phase 4: Cost Model (2-3 weeks)
**Deliverables:**
- JSON_TABLE cost function
- Statistics gathering for JSON columns
- Cardinality estimation

### Phase 5: Dialect Translation (2 weeks)
**Deliverables:**
- Translation to OPENJSON (SQL Server)
- Translation to FLATTEN (Snowflake)
- Cross-database compatibility tests

### Phase 6: Testing and Benchmarking (2-3 weeks)
**Deliverables:**
- 100+ integration tests
- Performance benchmark suite
- Benchmark report

**Total: 17-21 weeks (4-5 months)**

---

## Research Foundation

This RFC is based on comprehensive research from:

1. **SQL_STANDARDS_GAP_ANALYSIS.md**
   - Identified JSON_TABLE as highest-priority missing SQL:2016 feature
   - Documented 8/8 database support
   - Provided optimization opportunities analysis
   - Estimated 3-10x performance impact

2. **MYSQL_MARIADB_UNSUPPORTED_FEATURES.md**
   - Detailed MySQL 8.0 and MariaDB JSON functions
   - Multi-valued index support
   - Generated column patterns for JSON optimization
   - Cross-database JSON implementation differences

3. **Existing RFCs**
   - RFC 0083: XPath and XQuery Optimization (similar structural optimization)
   - RFC 0084: Oracle JSON Relational Duality View Optimization
   - RFC 0093: SQL Property Graph Queries (pattern matching)

---

## Key Decisions

### 1. Standard SQL:2016 Syntax
**Decision:** Implement standard JSON_TABLE syntax, not database-specific variants.

**Rationale:**
- Enables cross-database portability
- Easier for users to learn one syntax
- Optimization opportunities apply universally

### 2. Canonical Relational Algebra Representation
**Decision:** Single RelExpr::JsonTable variant with dialect translation.

**Rationale:**
- Optimization rules work across all databases
- Cleaner internal representation
- Dialect differences handled at translation layer

### 3. Cost-Based Optimization
**Decision:** Use cost model to choose between optimization strategies.

**Rationale:**
- Small JSON arrays: sequential unnesting
- Large JSON arrays (>1000 elements): parallel unnesting
- With indexes: index-based access
- Without statistics: conservative defaults

### 4. JSONPath Subset Support
**Decision:** Start with commonly-supported JSONPath subset, extend as needed.

**Initial support:**
- `$` - Root object
- `.field` - Object field access
- `[*]` - Array wildcard
- `[n]` - Array index
- `?(@.field > value)` - Filter expressions

**Rationale:**
- 90%+ of queries use basic JSONPath features
- Easier to implement and test
- Can extend to full JSONPath 1.0 in future

---

## Success Criteria

The implementation will be considered successful if it achieves:

1. **Correctness:** 100% of integration tests pass across all supported databases
2. **Performance:** Achieve 3-10x speedup on benchmark queries
3. **Coverage:** Support all major JSON_TABLE features (COLUMNS, NESTED PATH, error handling)
4. **Compatibility:** Work with PostgreSQL, MySQL, Oracle, SQL Server, Snowflake
5. **Documentation:** Complete user documentation with examples

---

## Next Steps

1. **Review RFC:** Gather feedback from Ra development team
2. **Approve RFC:** Get sign-off from project maintainers
3. **Create tracking issue:** Link RFC to GitHub issue
4. **Begin implementation:** Start with Phase 1 (parser support)
5. **Iterative development:** Implement in phases with testing at each stage

---

## Impact Assessment

### Benefits
✅ **High Impact:** JSON is ubiquitous in modern applications
✅ **Standard Compliance:** Implements SQL:2016 standard
✅ **Cross-Database Support:** Works with 8 major databases
✅ **Performance:** 3-10x speedup expected
✅ **Developer Experience:** Cleaner, more declarative syntax

### Risks
⚠️ **Medium Complexity:** 17-21 weeks implementation effort
⚠️ **Parser Complexity:** ~1500 lines of parser code
⚠️ **Cost Model Uncertainty:** JSON cardinality estimation without statistics
⚠️ **JSONPath Dialects:** Subtle differences across databases

### Mitigation
✓ Phased implementation with testing at each stage
✓ Start with JSONPath subset, extend as needed
✓ Conservative cost defaults, improve with statistics
✓ Cross-database integration tests

---

## Comparison with Other Missing Features

From SQL_STANDARDS_GAP_ANALYSIS.md priority ranking:

| Feature | Estimated Effort | Expected Impact | Priority | Status |
|---------|-----------------|-----------------|----------|--------|
| **JSON_TABLE** | 20-25 weeks | 3-10x speedup | HIGH | **RFC 0094 ✅** |
| GROUPING SETS/CUBE/ROLLUP | 25-30 weeks | N-1 scans saved | HIGH | Not started |
| PIVOT/UNPIVOT | 15-20 weeks | Cleaner syntax | HIGH | Not started |
| LATERAL Subqueries | 20-25 weeks | 10-100x speedup | HIGH | Not started |
| SQL/PGQ (Graph Queries) | 40-50 weeks | 10-100x speedup | MEDIUM | RFC 0093 |

**JSON_TABLE is the best effort-to-impact ratio among high-priority features.**

---

## Conclusion

RFC 0094 provides a complete, implementable design for JSON_TABLE support in Ra. The feature is:

- **Well-researched:** Based on analysis of 8 databases and SQL standards
- **Well-specified:** 1,098 lines covering all aspects of implementation
- **High-impact:** 3-10x optimization for JSON analytics workloads
- **Achievable:** 17-21 week implementation with clear phases
- **Standard-compliant:** Follows SQL:2016 specification

The RFC is ready for team review and approval to begin implementation.

---

**Document Status:** ✅ Complete
**Worktree:** `/home/gburd/ws/ra/.claude/worktrees/rfc-0094-json-table`
**Branch:** `rfc-0094-json-table`
**Commit:** `f6e2c18a feat: Add RFC 0094 for JSON_TABLE optimization`

**Next Action:** Review RFC with Ra development team

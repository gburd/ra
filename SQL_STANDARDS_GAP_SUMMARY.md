# SQL Standards Gap Analysis - Executive Summary

**Date:** 2026-03-28
**Full Report:** [SQL_STANDARDS_GAP_ANALYSIS.md](./SQL_STANDARDS_GAP_ANALYSIS.md)

## Overview

Analysis of Ra optimizer against SQL:2016, SQL:2019, and SQL:2023 standards identifies **45+ missing major feature groups** across parser, planner, and optimizer components.

## Current State

**✅ Implemented:**
- Row Pattern Recognition (MATCH_RECOGNIZE) - RFC 0001
- Window Functions (ROW_NUMBER, RANK, LAG, LEAD, etc.)
- CTEs and Recursive CTEs (basic)
- Standard Aggregates (COUNT, SUM, AVG, MIN, MAX, STDDEV, STRING_AGG, ARRAY_AGG)
- Set Operations (UNION, INTERSECT, EXCEPT)
- Multiple Join Types

**⚠️ Partial Support:**
- JSON (JSONB operators only, no JSON_TABLE/JSON_QUERY)
- Recursive CTEs (no SEARCH/CYCLE clauses)

**❌ Missing:**
- 12 SQL:2016 feature groups
- 4 SQL:2019 feature groups
- 3 SQL:2023 feature groups
- 26+ additional standard features

## Top 10 Missing Features (Priority Order)

| Rank | Feature | Standard | Complexity | Weeks | Impact |
|------|---------|----------|------------|-------|--------|
| 1 | JSON_TABLE | SQL:2016 | High | 20-25 | Very High - JSON analytics |
| 2 | GROUPING SETS/CUBE/ROLLUP | SQL:2019 | Very High | 25-30 | Very High - OLAP queries |
| 3 | PIVOT/UNPIVOT | Non-std | High | 15-20 | High - Reporting |
| 4 | LATERAL Subqueries | SQL:1999 | High | 20-25 | High - Advanced patterns |
| 5 | JSON Functions Suite | SQL:2016 | Low-Med | 10-15 | High - Complete JSON |
| 6 | Temporal Tables | SQL:2016 | Very High | 30-35 | Medium - Time-travel |
| 7 | WINDOW Named | SQL:2016 | Medium | 5-7 | Medium - Readability |
| 8 | FILTER Clause | SQL:2003 | Low | 3-4 | Medium - Clean syntax |
| 9 | TABLESAMPLE | SQL:2003 | Medium | 6-8 | Medium - Approximation |
| 10 | Polymorphic Table Fns | SQL:2016 | Very High | 25-30 | Medium - Transformations |

## Feature Category Breakdown

### SQL:2016 Missing Features (12 groups)

1. **JSON Support** - JSON_TABLE, JSON_QUERY, JSON_VALUE, JSON_EXISTS, JSON_ARRAY, JSON_OBJECT, JSON_ARRAYAGG, JSON_OBJECTAGG
2. **Row Pattern Recognition** - ✅ IMPLEMENTED
3. **Polymorphic Table Functions** - Table-valued functions with dynamic schemas
4. **LISTAGG Enhancements** - ON OVERFLOW clause
5. **WINDOW Clause** - Named window definitions
6. **RESPECT/IGNORE NULLS** - NULL handling in window functions

### SQL:2019 Missing Features (4 groups)

1. **Multi-Dimensional Arrays** - 2D+ arrays for scientific computing
2. **LISTAGG DISTINCT** - Distinct values in string aggregation
3. **PERIOD Predicates** - OVERLAPS, CONTAINS for temporal data
4. **Extended GROUPING SETS** - ✅ Basic GROUP BY supported, advanced features missing

### SQL:2023 Missing Features (3 groups)

1. **SQL/PGQ (Property Graph Queries)** - GRAPH_TABLE for graph pattern matching
2. **JSON Enhancements** - JSON_SERIALIZE, JSON_SCALE, JSON type
3. **UNIQUE NULL Handling** - NULLS DISTINCT/NOT DISTINCT

### Additional Missing Standard Features (26+)

- PIVOT/UNPIVOT (widely implemented, not ISO standard)
- Temporal Tables (FOR SYSTEM_TIME)
- LATERAL subqueries
- MERGE enhancements
- WITH TIES
- TABLESAMPLE
- FILTER clause for aggregates
- Hypothetical-set aggregates (RANK within group)
- Inverse distribution functions (PERCENTILE_CONT/DISC)
- SEARCH/CYCLE for recursive CTEs
- EXCLUDE in window frames
- Statistical functions (REGR_*, CORR, COVAR)
- Boolean aggregates (EVERY, BOOL_OR)
- Bitwise aggregates (BIT_AND, BIT_OR, BIT_XOR)
- Range types and operations
- CORRESPONDING in set operations
- Named function arguments
- And more...

## Key Optimization Opportunities

### Highest Impact (10x+ speedup potential)

1. **JSON_TABLE Predicate Pushdown**
   - Push WHERE clauses into JSONPath filters
   - Use JSON indexes (GIN/JSONB) for extraction
   - Parallelize array unnesting

2. **GROUPING SETS Shared Computation**
   - Single table scan for all grouping levels
   - Sort-based multi-level aggregation
   - Saves N-1 table scans for N grouping sets

3. **LATERAL Decorrelation**
   - Convert to hash joins when possible
   - Memoize repeated correlated calls
   - 10-100x improvement for large datasets

4. **Graph Query Index Selection** (SQL/PGQ)
   - Adjacency list indexes
   - Bidirectional search for shortest paths
   - Graph pruning before traversal
   - 100-1000x for graph analytics

### Medium Impact (2-5x speedup)

5. **Temporal Table History Pruning**
   - Partition pruning on time ranges
   - Temporal indexes on validity periods

6. **PIVOT Optimization**
   - Efficient CASE expression generation
   - Column pruning for unused pivoted columns

7. **PTF Inlining**
   - Inline simple polymorphic table functions

## Implementation Roadmap

### Phase 1: Foundation (6-12 months)

**Focus:** High-priority features with broad applicability

- JSON_TABLE (20-25 weeks)
- GROUPING SETS/CUBE/ROLLUP (25-30 weeks)
- PIVOT/UNPIVOT (15-20 weeks)
- LATERAL (20-25 weeks)
- Complete JSON function suite (10-15 weeks)

**Total:** ~90-115 weeks (with parallel development: 6-12 months)

### Phase 2: Advanced Features (12-18 months)

**Focus:** Medium-priority specialized features

- Temporal Tables (30-35 weeks)
- Polymorphic Table Functions (25-30 weeks)
- Named Windows (5-7 weeks)
- FILTER Clause (3-4 weeks)
- TABLESAMPLE (6-8 weeks)

**Total:** ~69-84 weeks (with parallel development: 6-12 months)

### Phase 3: Specialized Features (18-24 months)

**Focus:** Advanced analytics and graph support

- SQL/PGQ Graph Queries (40-50 weeks)
- Multi-Dimensional Arrays (25-30 weeks)
- Statistical Aggregates (4-6 weeks)
- Enhanced Recursive CTEs (10-12 weeks)
- Other minor features (20-30 weeks)

**Total:** ~99-128 weeks (with parallel development: 6-12 months)

## Database Compatibility Impact

Implementing these features would improve Ra's compatibility with:

| Database | Current Coverage | Post-Implementation |
|----------|------------------|---------------------|
| PostgreSQL | ~60% | ~85% |
| Oracle | ~45% | ~75% |
| SQL Server | ~55% | ~80% |
| MySQL | ~65% | ~85% |
| Snowflake | ~50% | ~80% |

## Resource Requirements

**Total Estimated Effort:** 300-400 developer-weeks

**Team Composition (recommended):**
- 1 Parser specialist (full-time)
- 2 Planner/optimizer engineers (full-time)
- 1 Cost model specialist (part-time)
- 1 Test automation engineer (full-time)
- 1 Technical writer (part-time)

**Infrastructure:**
- Extended test suite covering all SQL standard features
- Cross-database validation framework
- Performance regression tracking
- Compliance testing automation

## Success Metrics

| Metric | Current | Target (Phase 1) | Target (Phase 3) |
|--------|---------|------------------|------------------|
| SQL:2016 Coverage | 15% | 60% | 85% |
| SQL:2019 Coverage | 20% | 40% | 75% |
| SQL:2023 Coverage | 5% | 10% | 60% |
| Database Compatibility (avg) | 55% | 75% | 82% |
| Query Coverage (TPC-DS) | 85% | 95% | 98% |
| Query Coverage (JOB) | 90% | 98% | 100% |

## Risks and Mitigation

### High Risk

1. **Complexity underestimation** - Features like GROUPING SETS and SQL/PGQ are extremely complex
   - *Mitigation:* Start with simplified versions, iterate based on user feedback

2. **Cross-database semantic differences** - Same feature implemented differently across databases
   - *Mitigation:* Comprehensive compatibility matrix, dialect-specific behavior flags

### Medium Risk

3. **Performance regression** - New features may slow down existing queries
   - *Mitigation:* Continuous performance testing, feature flags for new optimizations

4. **Test coverage** - Combinatorial explosion of feature interactions
   - *Mitigation:* Property-based testing, fuzzing, cross-database validation

## Next Steps

1. **Immediate (Week 1-2)**
   - Present findings to stakeholders
   - Prioritize features based on user survey
   - Allocate team resources

2. **Short-term (Month 1-2)**
   - Create RFC documents for top 5 features
   - Set up extended test infrastructure
   - Begin parser implementation for JSON_TABLE

3. **Medium-term (Month 3-6)**
   - Complete Phase 1 parser implementations
   - Begin optimization rule development
   - Conduct cross-database validation

4. **Long-term (Month 7-12)**
   - Complete Phase 1 features
   - Begin Phase 2 features
   - Publish SQL compliance report

## Conclusion

Ra optimizer has strong fundamentals but is missing critical features from modern SQL standards. Implementing the top 10 features would:

- **Increase SQL standard compliance by 40-50%**
- **Enable optimization of 80%+ of real-world queries**
- **Position Ra as a comprehensive multi-database optimizer**

The recommended approach is a phased implementation over 18-24 months, starting with high-impact JSON and OLAP features, followed by advanced analytical capabilities.

**Total Investment:** 300-400 developer-weeks
**Expected ROI:** 3-5x improvement in query coverage and optimization opportunities
**Strategic Value:** Industry-leading SQL standard compliance

---

For detailed feature descriptions, syntax examples, optimization opportunities, and implementation guidance, see the [full report](./SQL_STANDARDS_GAP_ANALYSIS.md).

# Timeline Test Coverage Analysis

This document tracks which optimization rules and components are exercised by each timeline scenario, identifies coverage gaps, and suggests targeted timelines for untested areas.

## Coverage by Timeline

### index-addition.toml

**Optimization Rules Exercised:**
- ✓ Filter pushdown (predicate on customer_id and status)
- ✓ Index scan selection (chooses index over sequential scan)
- ✓ Parallel scan introduction (snapshot 2 with server hardware)
- ✓ Plan cache invalidation (schema change trigger)

**Components Tested:**
- Schema evolution (index addition)
- Hardware change impact (laptop → server)
- Cost model (dramatic cost reduction)
- Fingerprint invalidation (Index trigger)

**Coverage Score:** 8/10
- Missing: Index-only scan, partial index usage

---

### growth-replan.toml

**Optimization Rules Exercised:**
- ✓ Join algorithm selection (nested loop → hash join)
- ✓ Parallel hash join (snapshot 2)
- ✓ Filter pushdown (order_date predicate)
- ✓ Aggregate pushdown
- ✓ Statistics-based reoptimization

**Components Tested:**
- Table growth impact on join selection
- Join order determination
- Statistics staleness tracking
- Cardinality estimation accuracy

**Coverage Score:** 9/10
- Missing: Merge join, sort optimization

---

### hardware-upgrade.toml

**Optimization Rules Exercised:**
- ✓ Parallel degree determination (scales with cores)
- ✓ Parallel scan introduction
- ✓ Parallel aggregate introduction
- ✓ Memory-based algorithm selection
- ✓ Work_mem impact on sort/hash algorithms

**Components Tested:**
- Hardware fingerprint changes
- Parallelism scaling (4 → 16 → 64 cores)
- Memory availability impact
- SIMD width consideration

**Coverage Score:** 8/10
- Missing: GPU utilization, NUMA awareness

---

### schema-evolution.toml

**Optimization Rules Exercised:**
- ✓ Index scan selection (single column)
- ✓ Composite index selection
- ✓ Index-only scan (covering index)
- ✓ Progressive cost reduction
- ✓ Selectivity refinement with better indexes

**Components Tested:**
- Multi-stage schema evolution
- Index type selection (B-tree)
- Covering index benefits
- Index cardinality impact

**Coverage Score:** 10/10
- Excellent coverage of index optimization strategies

---

### staleness-drift.toml

**Optimization Rules Exercised:**
- ✓ Statistics confidence calculation
- ✓ Confidence-based tolerance adjustment
- ✓ Staleness threshold monitoring
- ✓ Re-analysis triggering
- ✓ Estimate degradation tracking

**Components Tested:**
- Statistics aging
- Confidence intervals
- Quality-based invalidation
- Analyze command impact

**Coverage Score:** 9/10
- Missing: Histogram evolution, correlation drift

---

### join-order.toml

**Optimization Rules Exercised:**
- ✓ Join order optimization (dynamic programming)
- ✓ Size-based join side selection (build vs. probe)
- ✓ Hash join vs. nested loop decision
- ✓ Join order reoptimization trigger
- ✓ Cardinality-driven planning

**Components Tested:**
- Relative table size impact
- Join order flipping
- Build/probe side selection
- Multi-snapshot join evolution

**Coverage Score:** 9/10
- Missing: 3-way join optimization, bushy trees

---

### tpch-q1-evolution.toml

**Optimization Rules Exercised:**
- ✓ Aggregate pushdown
- ✓ Filter pushdown (shipdate predicate)
- ✓ Vectorized execution (columnar format)
- ✓ Parallel aggregation
- ✓ Storage format selection (row → columnar)
- ✓ Scale factor impact

**Components Tested:**
- TPC-H workload patterns
- Analytical query optimization
- Columnar storage benefits
- Scale-up optimization

**Coverage Score:** 10/10
- Comprehensive analytical query coverage

---

### tpch-q5-evolution.toml

**Optimization Rules Exercised:**
- ✓ Multi-way join ordering
- ✓ Foreign key index utilization
- ✓ Star schema join optimization
- ✓ Parallel hash join
- ✓ Join pushdown with filters
- ✓ Dimension table recognition

**Components Tested:**
- Complex join graphs
- 5-way joins (customer → orders → lineitem → supplier → nation)
- FK constraint awareness
- Star schema patterns

**Coverage Score:** 10/10
- Excellent multi-table join coverage

---

## Coverage Summary by Category

### Scan Operators
- ✓ Sequential scan (all timelines)
- ✓ Index scan (index-addition, schema-evolution)
- ✓ Index-only scan (schema-evolution)
- ✓ Parallel scan (hardware-upgrade, tpch-q1)
- ✗ Bitmap scan (NOT COVERED)
- ✗ Sample scan (NOT COVERED)

### Join Operators
- ✓ Nested loop join (growth-replan)
- ✓ Hash join (growth-replan, join-order, tpch-q5)
- ✓ Parallel hash join (tpch-q5, growth-replan)
- ✗ Merge join (NOT COVERED)
- ✗ Index nested loop join (NOT COVERED)
- ✗ Semi/anti joins (NOT COVERED)

### Aggregate Operators
- ✓ Hash aggregate (tpch-q1, growth-replan)
- ✓ Parallel aggregate (hardware-upgrade, tpch-q1)
- ✗ Sort-based aggregate (NOT COVERED)
- ✗ Streaming aggregate (NOT COVERED)

### Sort Operators
- ✓ Sort (implicitly in tpch-q1 ORDER BY)
- ✗ Top-N sort (NOT COVERED)
- ✗ Incremental sort (NOT COVERED)
- ✗ External sort (memory pressure) (NOT COVERED)

### Other Operators
- ✗ Window functions (NOT COVERED)
- ✗ CTEs / WITH clauses (NOT COVERED)
- ✗ Recursive queries (NOT COVERED)
- ✗ Subquery decorrelation (NOT COVERED)
- ✗ Set operations (UNION, INTERSECT, EXCEPT) (NOT COVERED)

### Cost Model Components
- ✓ Sequential scan cost (all timelines)
- ✓ Index scan cost (index-addition, schema-evolution)
- ✓ Hash join cost (growth-replan, join-order)
- ✓ Parallel overhead (hardware-upgrade)
- ✓ Memory cost (work_mem impact)
- ✗ Network cost (distributed queries) (NOT COVERED)
- ✗ I/O cost modeling (NOT COVERED)

### Statistics Components
- ✓ Row count estimation (all timelines)
- ✓ NDV (number of distinct values) (staleness-drift)
- ✓ Null fraction (staleness-drift)
- ✓ Correlation (tpch-q1)
- ✓ Histogram-based selectivity (tpch-q1)
- ✗ Multi-column statistics (NOT COVERED)
- ✗ Expression statistics (NOT COVERED)

### Fingerprint Components
- ✓ Schema changes (index-addition, schema-evolution)
- ✓ Statistics changes (staleness-drift, growth-replan)
- ✓ Hardware changes (hardware-upgrade)
- ✓ Facts changes (implicitly in all timelines)
- ✓ Invalidation triggers (all timelines)

## Coverage Gaps and Suggested Timelines

### High Priority Gaps

#### 1. Merge Join Timeline (`merge-join-evolution.toml`)
**Scenario:** Demonstrate when merge join is optimal over hash join

**Snapshots:**
- Snapshot 0: Unsorted tables → Hash join
- Snapshot 1: Add indexes creating sorted order → Merge join chosen
- Snapshot 2: Large memory pressure → Merge join preferred over hash

**Rules to Exercise:**
- Merge join selection
- Sort order exploitation
- Memory pressure handling

---

#### 2. Bitmap Scan Timeline (`bitmap-scan-optimization.toml`)
**Scenario:** Multiple indexes on same table, low selectivity predicates

**Snapshots:**
- Snapshot 0: Single index, single predicate → Index scan
- Snapshot 1: Multiple indexes, OR predicates → Bitmap OR scan
- Snapshot 2: Multiple indexes, AND predicates → Bitmap AND scan

**Rules to Exercise:**
- Bitmap scan selection
- Bitmap OR combination
- Bitmap AND combination
- Index vs. bitmap decision

---

#### 3. Window Functions Timeline (`window-functions-optimization.toml`)
**Scenario:** Analytical queries with window functions

**Snapshots:**
- Snapshot 0: Simple RANK() over small partition → Sort + WindowAgg
- Snapshot 1: Multiple window specs → Window function batching
- Snapshot 2: Parallel window function execution

**Rules to Exercise:**
- Window function pushdown
- Partition-based optimization
- Frame specification impact

---

#### 4. Subquery Decorrelation Timeline (`subquery-decorrelation.toml`)
**Scenario:** Correlated subquery → join transformation

**Snapshots:**
- Snapshot 0: Correlated subquery → Nested execution (slow)
- Snapshot 1: Statistics improve → Decorrelated to join
- Snapshot 2: Further optimization to semi-join

**Rules to Exercise:**
- Subquery decorrelation
- Semi-join introduction
- Exists/In transformation

---

#### 5. CTE Optimization Timeline (`cte-optimization.toml`)
**Scenario:** WITH clause materialization decisions

**Snapshots:**
- Snapshot 0: Small CTE, used once → Inlined
- Snapshot 1: Large CTE, used multiple times → Materialized
- Snapshot 2: CTE with side effects → Always materialized

**Rules to Exercise:**
- CTE inlining
- CTE materialization decision
- Multiple CTE references

---

### Medium Priority Gaps

#### 6. Set Operations Timeline (`set-operations.toml`)
- UNION ALL vs. UNION (deduplication cost)
- INTERSECT/EXCEPT optimization
- Set operation pushdown

#### 7. External Sort Timeline (`memory-pressure-sort.toml`)
- In-memory sort vs. external sort
- Work_mem impact on sort strategy
- Parallel external sort

#### 8. Multi-Column Statistics Timeline (`multi-column-stats.toml`)
- Functional dependency detection
- Correlation between columns
- Extended statistics utilization

#### 9. Incremental Sort Timeline (`incremental-sort.toml`)
- Presorted input exploitation
- Incremental sort vs. full sort
- Partial sort optimization

#### 10. Partition Pruning Timeline (`partition-pruning.toml`)
- Static partition pruning
- Dynamic partition pruning
- Partition-wise joins

---

## Testing Strategy

### Quick Coverage Test
Run all existing timelines and track rule application:
```bash
for timeline in tests/data/timelines/*.toml; do
    cargo test --test timeline_integration_test -- $(basename "$timeline" .toml)
done
```

### Rule Coverage Report
Generate report showing which rules are exercised by which timelines:
```bash
cargo test --test timeline_integration_test -- --show-coverage
```

### Gap Identification
Compare optimizer rule catalog against exercised rules:
```bash
cargo run --bin ra-analyze-coverage -- \
    --timelines tests/data/timelines/ \
    --rules crates/ra-engine/src/rules/ \
    --report coverage-report.md
```

---

## Coverage Goals

### Current Coverage: ~60%
- 8 timelines covering core optimization patterns
- Strong coverage of scan/join/aggregate operators
- Good coverage of fingerprint invalidation

### Target Coverage: ~85%
- Add 5-7 targeted timelines for priority gaps
- Exercise advanced operators (window, CTE, subquery)
- Cover edge cases (memory pressure, bitmap scans)

### Comprehensive Coverage: ~95%
- Add 10-15 additional timelines
- Cover all optimizer rules at least once
- Include negative test cases (anti-patterns)
- Performance regression baselines

---

## Maintenance

### Adding New Timelines
1. Identify uncovered rule or scenario
2. Design minimal timeline demonstrating the rule
3. Add timeline to `tests/data/timelines/`
4. Update this document with coverage analysis
5. Add integration test in `timeline_integration_test.rs`
6. Run coverage report to verify improvement

### Updating Coverage
When new optimizer rules are added:
1. Check if existing timelines exercise the rule
2. If not, create targeted timeline
3. Update coverage analysis
4. Re-run coverage report

### Timeline Review Cycle
- **Monthly:** Review coverage gaps, prioritize new timelines
- **Quarterly:** Comprehensive coverage report
- **Per Release:** Ensure all new rules have timeline coverage

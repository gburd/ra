# MonetDB Gap Analysis - Executive Summary

**Date:** 2026-03-28
**Report:** MONETDB_FEATURES_ANALYSIS.md

## Quick Stats

- **Total MonetDB Features Analyzed:** 45
- **Ra Support:** 23 fully supported (51%), 13 partially supported (29%), 12 missing (20%)
- **Existing MonetDB Rules in Ra:** 28 rules (3,691 lines)
- **Rule Coverage:** Strong coverage of production features, gaps in research-oriented optimizations

## Coverage Heatmap

```
✅✅✅✅✅  Column-Store Optimizations (5/5)
✅✅✅⚠️    BAT Algebra & Imprints (8/9)
✅✅⚠️     Database Cracking (2/3)
✅✅✅     Parallelism (Mitosis, Tail Ordering) (3/3)
⚠️⚠️      Vectorization & Selection Vectors (1/2)
✅⚠️      Sideways Information Passing (2/3)
❌❌      Approximate Query Processing (0/2)
❌❌      R/Python UDF Integration (0/1)
❌       Streaming Queries (0/1)
```

## Top 5 Missing Features with High Impact

### 1. Runtime Sideways Information Passing ⭐⭐⭐⭐⭐
**Impact:** 2-10x speedup for complex joins
**Complexity:** Very High (requires execution engine integration)
**Applicability:** Any database with adaptive execution
**Status in Ra:** Partial (static bloom filter pushdown exists, not runtime-generated)

**What it is:** MonetDB passes statistics between operators **during execution**, not just at planning time. Bloom filters generated during hash join build phase are pushed to scan operators mid-execution.

**Why it matters:** Fixes the #1 optimizer failure mode: bad cardinality estimates. No amount of pre-planning can match actual runtime statistics.

**Ra's path forward:** Extend RFC 0052 (progressive re-optimization) with operator-level feedback loops.

---

### 2. Selection Vector Propagation ⭐⭐⭐⭐
**Impact:** 2-5x memory bandwidth reduction
**Complexity:** Low (metadata tracking only)
**Applicability:** DuckDB, ClickHouse, any vectorized engine
**Status in Ra:** Missing

**What it is:** After a selective filter, maintain a bitmap/index list of valid positions instead of compacting data. Subsequent operators use selection vector to skip invalid entries.

**Why it matters:** Vectorized engines materialize intermediate vectors between operators. For selective queries (e.g., 5% pass rate), 95% of data is dead weight. Selection vectors eliminate copying.

**Ra's path forward:** Add cost model comparing selection vector overhead vs compaction cost. Rule applicability: selectivity < 30%, vectorized execution.

---

### 3. Approximate Query Processing ⭐⭐⭐⭐
**Impact:** 10-1000x speedup for exploratory analytics
**Complexity:** High (requires sampling metadata, confidence intervals)
**Applicability:** BigQuery, Redshift, Snowflake (interactive analytics)
**Status in Ra:** Missing

**What it is:** Return approximate results using statistical samples. Trade accuracy for speed.

**Why it matters:** Enables sub-second analytics on TB+ datasets. "How many users logged in last week?" doesn't need exact count for exploration.

**Ra's path forward:** Add reservoir sampling rules, sketch-based aggregates (HyperLogLog for COUNT DISTINCT, t-digest for percentiles), sample-aware plan selection.

---

### 4. Morsel-Driven Parallelism ⭐⭐⭐
**Impact:** 10-30% latency reduction for skewed workloads
**Complexity:** Medium (requires work queue infrastructure)
**Applicability:** HyPer, DuckDB, Umbra
**Status in Ra:** Partial (tests exist, no cost models)

**What it is:** Divide work into fine-grained morsels (~10K tuples), use work-stealing queue for load balancing.

**Why it matters:** Static partitioning (mitosis) leaves cores idle when partitions have unequal work. Morsel-driven execution achieves near-perfect load balance.

**Ra's path forward:** Ra has morsel execution tests. Add rules for morsel size tuning, work-stealing cost models.

---

### 5. Positional/Co-Partitioned Join ⭐⭐⭐
**Impact:** 10x faster than hash join for aligned columns
**Complexity:** Medium (requires alignment metadata)
**Applicability:** Star schema denormalization, broadcast joins
**Status in Ra:** Missing

**What it is:** For columns with aligned positions (from same table or co-partitioned), use O(n) lockstep iteration instead of hash join.

**Why it matters:** Common in star schema queries after dimension table broadcast. No hash table build, no probe phase.

**Ra's path forward:** Generalize MonetDB positional join to "partition-aligned join". Detect: columns from same table, or co-partitioned distributed tables.

---

## Low-Priority Research Features (Defer)

1. **Advanced Cracking Strategies** (sideways cracking, hybrid cracking)
   - Research-only (not in production MonetDB)
   - Database cracking is MonetDB-specific (other DBs use pre-built indexes)

2. **MAL Instruction-Level Optimization**
   - MonetDB-specific internal IR
   - Ra operates at relational algebra level, not assembly

3. **Partial Computation Reuse** (query containment detection)
   - Very high complexity
   - Research prototype (Ivanova et al. CWI)

4. **SciQL / Multidimensional Arrays**
   - Niche domain (scientific computing)
   - Users migrated to specialized systems (SciDB, TileDB)

5. **Streaming Continuous Queries**
   - Different execution model from batch queries
   - Out of scope for Ra (batch query optimizer)

---

## Key Insights for Ra Development

### 1. MonetDB's Winning Strategies (Ra Should Adopt)

**Zero-Maintenance Auxiliary Structures:**
- Imprints, zone maps require no maintenance (embedded in storage)
- **Ra Lesson:** Prefer optimizations with no maintenance cost over B-tree indexes

**Column-at-a-Time Cost Models:**
- MonetDB reasons about OID vector sizes, not row counts
- **Ra Lesson:** Extend cost models for columnar engines to consider column projection, compression, SIMD width

**Adaptive Convergence:**
- Database cracking, stochastic optimization
- **Ra Lesson:** Track query patterns, adapt structures incrementally (already started with RFC 0014 index recommendations)

**Sideways Information Passing:**
- Operators share statistics during execution
- **Ra Lesson:** Progressive re-optimization (RFC 0052) is the right direction. Extend with operator feedback.

---

### 2. Ra's Competitive Advantages Over MonetDB

**Cross-Database Optimization:**
- Ra rules apply to 20+ databases
- MonetDB optimizations are MonetDB-specific

**Formal Verification:**
- Ra has TLA+ specifications
- MonetDB optimizations are ad-hoc C code

**Extensibility:**
- Ra's `.rra` literate format makes rules accessible
- MonetDB's MAL optimizer is 100K+ lines of C

**Rule Composability:**
- Ra's equality saturation explores all equivalent plans
- MonetDB uses greedy heuristics

---

### 3. MonetDB's Competitive Advantages Over Ra

**Integrated Execution:**
- MonetDB's optimizer tightly couples with execution engine (MAL)
- Ra optimizes relational algebra, delegates execution

**Runtime Adaptation:**
- MonetDB's sideways information passing happens during execution
- Ra re-optimizes between executions (working to close this gap)

**Research Innovation:**
- MonetDB pioneered database cracking, imprints, X100 vectorization
- Ra codifies existing techniques (not inventing new ones)

**Domain-Specific Optimizations:**
- MonetDB has 25+ years of column-store optimizations
- Ra is generalist (broader but shallower)

---

## Action Items for Ra

### Immediate (Low Effort, High Impact)

- [ ] Add selection vector propagation rules
- [ ] Add positional/co-partitioned join rules
- [ ] Add bit-packing/FOR encoding rules (generic, not MonetDB-specific)

### Medium-Term (Moderate Effort, High Impact)

- [ ] Extend RFC 0052 with runtime bloom filter generation
- [ ] Add operator selectivity feedback to cost model
- [ ] Add morsel-driven parallelism cost models

### Long-Term Research Integration

- [ ] Approximate query processing (reservoir sampling, sketches)
- [ ] Query containment detection for computation reuse
- [ ] Full-text search optimization (inverted indexes)

---

## Conclusion

**Ra has excellent coverage of MonetDB's production features** (80% full or partial support). The 28 existing MonetDB-specific rules capture the core column-store optimizations.

**Missing features are either:**
1. **High-value, broadly applicable** → Add to Ra (selection vectors, runtime adaptation, AQP)
2. **MonetDB-specific, low transferability** → Skip (MAL optimizations, advanced cracking)
3. **Research-only** → Defer (partial computation reuse, SciQL, streaming)

**Recommended focus:** Tier 1 features (selection vectors, runtime adaptation, morsel parallelism, approximate queries). These address major pain points (estimation errors, skewed workloads, interactive analytics) and apply broadly beyond MonetDB.

**MonetDB's most transferable innovations:**
- X100 vectorization → Already adopted by DuckDB, Snowflake, ClickHouse
- Imprints/zone maps → Similar to Parquet row groups, BRIN indexes
- Sideways information passing → Active research area (HyPer, SQL Server adaptive joins)
- Database cracking → Unique to MonetDB (other DBs use pre-built indexes)

**Ra's role:** Codify and formalize these techniques as reusable optimization rules with clear preconditions, cost models, and applicability across multiple databases.

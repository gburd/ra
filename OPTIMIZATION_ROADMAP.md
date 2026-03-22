# Ra Optimizer Performance Roadmap

**Goal**: Close the **20-150x performance gap** between Ra and PostgreSQL
**Current**: Ra takes ~1.1s to optimize 5-7 table joins
**Target**: <100ms for simple queries, <500ms for complex queries
**Status**: 15 tasks created, dependencies mapped

---

## Performance Targets

| Query Type | Current | Target | Improvement Needed |
|------------|---------|--------|--------------------|
| Simple (2-4 tables) | ~1000ms | <50ms | **20x faster** |
| Medium (5-7 tables) | ~1100ms | <100ms | **11x faster** |
| Complex (8+ tables) | ~1200ms | <500ms | **2.4x faster** |

---

## Optimization Strategy

### Phase 1: Measure & Understand (Week 1)

**Goal**: Identify where Ra is spending time

**Tasks**:
1. **#241: Profile with flamegraph** 🔥 (PRIORITY 1)
   - Run: `cargo flamegraph --bench job_benchmark -- --bench`
   - Identify top 5 hot paths
   - Document findings
   - **Blocks**: All optimization tasks

2. **#247: Test all 113 JOB queries**
   - Validate correctness
   - Ensure optimizations don't break queries
   - **Can run in parallel with profiling**

**Expected outcome**: Know exactly what to optimize

---

### Phase 2: Quick Wins (Week 2)

**Goal**: Implement low-hanging fruit optimizations

**Tasks** (Priority order):

1. **#242: Optimization timeout** (CRITICAL)
   - Add 100ms/500ms timeout limits
   - Prevent runaway optimization
   - **Production safety essential**
   - Expected: 1x speedup (but prevents hangs)

2. **#243: Cache statistics lookups**
   - Depends on: #241 (profiling)
   - Avoid repeated table stat lookups
   - Expected: **2-5x speedup** if stats are bottleneck

3. **#248: Left-deep tree for simple queries**
   - Fast path for 2-4 table queries
   - Skip e-graph entirely
   - Expected: **10-50x speedup** for simple queries

4. **#254: Query complexity classifier**
   - Route queries to appropriate strategy
   - Enables adaptive optimization
   - Foundation for other optimizations

**Expected outcome**: 5-10x overall improvement

---

### Phase 3: Core Optimizations (Week 3-4)

**Goal**: Attack fundamental bottlenecks

**Tasks**:

1. **#244: Early plan pruning**
   - Depends on: #241 (profiling)
   - Prune bad plans early
   - Expected: **3-10x speedup**

2. **#246: Reduce e-graph iterations**
   - Adaptive iteration limits
   - Simple queries: 5 iterations
   - Complex queries: 20 iterations
   - Expected: **5-10x speedup** for simple queries

3. **#245: Greedy fallback (RFC 0017)**
   - For 8+ table joins
   - O(n²) vs exponential
   - Expected: **10-100x speedup** for large joins

4. **#253: Optimize hot paths**
   - Depends on: #241 (profiling)
   - Target specific slow functions
   - Expected: **2-5x per hot path**

**Expected outcome**: Reach or exceed target performance

---

### Phase 4: Advanced Features (Month 2)

**Goal**: Implement sophisticated optimization techniques

**Tasks**:

1. **#249: Metrics and logging**
   - Instrumentation for monitoring
   - Debug optimization decisions
   - Track regressions

2. **#251: Expand benchmark coverage**
   - From 5 to 20+ queries
   - Broader coverage
   - Identify edge cases

3. **#250: Progressive optimization (RFC 0052)**
   - Return quick plan immediately
   - Optimize in background
   - Advanced feature

**Expected outcome**: Production-ready optimizer

---

### Phase 5: Stabilize & Monitor (Ongoing)

**Goal**: Maintain performance, prevent regressions

**Tasks**:

1. **#252: CI integration**
   - Depends on: Stabilized performance
   - Automated regression detection
   - Fail CI on >20% slowdown

2. **#255: Document results**
   - Depends on: Optimizations complete
   - Before/after comparison
   - Lessons learned

**Expected outcome**: Sustainable performance

---

## Task Dependency Graph

```
#241 (Profile) ──┬──> #243 (Cache stats)
                 ├──> #244 (Pruning)
                 └──> #253 (Optimize hot paths) ──> #255 (Document)

#242 (Timeout) ──────────────────────> [Production safety]

#248 (Left-deep) ──┐
#245 (Greedy)      ├──> #252 (CI integration)
#243 (Cache)       │
#244 (Pruning) ────┘

#247 (Test 113 queries) ──> [Correctness validation]

#254 (Classifier) ──> [Enables adaptive strategy]

#246 (Iterations) ──> [E-graph optimization]

#249 (Metrics) ──> [Observability]

#250 (Progressive) ──> [Advanced feature]

#251 (Expand benchmark) ──> [Better coverage]
```

---

## Expected Speedup by Technique

| Optimization | Expected Speedup | Confidence | Effort |
|--------------|------------------|------------|--------|
| Left-deep trees (simple) | 10-50x | High | Medium |
| Statistics caching | 2-5x | Medium | Low |
| Early pruning | 3-10x | High | Medium |
| E-graph iteration limits | 5-10x | High | Low |
| Greedy fallback (complex) | 10-100x | High | High |
| Hot path optimization | 2-5x per path | Varies | Varies |
| Timeout mechanism | 1x | High | Low |

**Cumulative (if independent)**: Could achieve 50-100x speedup
**Realistic (with overlap)**: Expect **10-20x overall speedup**

---

## Success Criteria

### Minimum Viable (Phase 2 End)

- ✅ Simple queries: <200ms (10x improvement from ~1s)
- ✅ Timeout prevents runaway optimization
- ✅ No correctness regressions

### Target (Phase 3 End)

- ✅ Simple queries: <100ms (11x improvement)
- ✅ Complex queries: <500ms (2.4x improvement)
- ✅ All 113 JOB queries pass

### Stretch (Phase 4 End)

- ✅ Simple queries: <50ms (20x improvement)
- ✅ Complex queries: <200ms (6x improvement)
- ✅ Progressive optimization working
- ✅ CI integration complete

---

## Risk Mitigation

### Risk 1: Profiling shows no single bottleneck

**Mitigation**: Focus on overall strategy changes (left-deep, greedy) rather than micro-optimizations

### Risk 2: E-graph library fundamentally slow

**Mitigation**: Implement alternative optimization strategies (greedy, left-deep) that bypass e-graph

### Risk 3: Optimizations break correctness

**Mitigation**: Task #247 (test all 113 queries) runs continuously, CI catches regressions

### Risk 4: Can't reach target performance

**Mitigation**:
- Phase 2 quick wins should give 5-10x
- If not enough, implement multiple Phase 3 optimizations
- Fallback: Adjust targets based on realistic achievable performance

---

## Timeline

| Phase | Duration | Start | End | Deliverable |
|-------|----------|-------|-----|-------------|
| Phase 1 | 1 week | Now | +1w | Profiling results, 113 query tests |
| Phase 2 | 1 week | +1w | +2w | 5-10x speedup, timeout implemented |
| Phase 3 | 2 weeks | +2w | +4w | Target performance achieved |
| Phase 4 | 3 weeks | +4w | +7w | Production-ready optimizer |
| Phase 5 | Ongoing | +7w | Forever | Maintained performance |

**Total to target performance**: ~4 weeks
**Total to production-ready**: ~7 weeks

---

## Monitoring & Iteration

### After Each Optimization

1. Run benchmarks: `cargo bench --bench job_benchmark`
2. Compare to baseline
3. Document speedup achieved
4. If target not met, profile again and repeat

### Weekly Reviews

- Review progress toward targets
- Adjust priorities based on results
- Identify blockers and risks

### Metrics to Track

- **Optimization time** (by query complexity)
- **Speedup factor** (vs baseline)
- **Correctness** (% of JOB queries passing)
- **Cache hit rates**
- **Timeout frequency**

---

## References

- **Baseline**: `JOB_BENCHMARK_FINDINGS.md` - Current performance
- **RFCs**:
  - RFC 0017: Large Join Graph Fallback (greedy algorithm)
  - RFC 0052: Progressive Re-Optimization
- **Benchmarks**: `crates/ra-engine/benches/job_benchmark.rs`
- **Tasks**: See TaskList for full details

---

## Next Action

🔥 **START HERE**: Task #241 - Profile Ra optimizer

```bash
cargo install flamegraph
cargo flamegraph --bench job_benchmark -- --bench
```

This will generate `flamegraph.svg` showing where Ra spends time.

Once profiling is done, priorities will become clear!

---

**Updated**: March 22, 2026
**Status**: Roadmap defined, ready to execute
**Next Review**: After profiling complete (Task #241)

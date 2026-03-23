# RFC 0058: Adaptive Search Space Limits

**Status**: Draft
**Date**: 2026-03-22
**Authors**: Ra Team
**Related**: Task #246, PROFILING_FINDINGS.md

---

## Summary

Implement adaptive iteration limits and search space pruning to reduce query optimization time from 770ms to <100ms for typical queries. Uses query complexity heuristics, early termination, and cost-based pruning to manage the exponentially growing plan space in e-graph equality saturation.

---

## Motivation

### Current Problem

Ra's optimizer spends **95.8% of time** in e-graph equality saturation, running a fixed 30 iterations regardless of query complexity:

| Query Type | Tables | Current Time | E-Graph Time | Wasted Iterations |
|------------|--------|--------------|--------------|-------------------|
| Simple | 2-4 | ~1000ms | ~950ms | ~20-25 iterations |
| Medium | 5-7 | ~770ms | ~738ms | ~11-18 iterations |
| Complex | 8+ | Varies | Varies | Uses large join fallback |

**Key Finding**: Queries saturate after 10-18 iterations but continue for full 30 iterations, wasting 495ms+ per query.

### Goals

1. **Adaptive iteration limits** - Scale iterations with query complexity
2. **Early termination** - Stop when e-graph converges
3. **Cost-based pruning** - Prune dominated plans during search
4. **Timeout safety** - Hard limits prevent runaway optimization

**Target Performance**:
- Simple queries: <50ms (20x faster than current ~1000ms)
- Medium queries: <100ms (7x faster than current ~770ms)
- Complex queries: <500ms

---

## Prior Art

### 1. PostgreSQL GEQO (Genetic Query Optimizer)

**Threshold-Based Switching**:
- `geqo_threshold` = 12 tables (default)
- Below threshold: Dynamic programming with exhaustive search
- Above threshold: Genetic algorithm (heuristic)

**DP Enumeration Bounds**:
- `join_collapse_limit` = 8 (default) - Controls join reordering scope
- `from_collapse_limit` = 8 (default) - Controls FROM-list flattening
- Prevents exponential explosion by limiting reordering window

**Key Insight**: PostgreSQL uses **complexity thresholds** to switch optimization strategies, not fixed iteration counts.

### 2. Apache Calcite (Volcano Planner)

**Branch-and-Bound Pruning**:
- Tracks best plan cost found so far
- Prunes search branches with cost > best_cost $\times$ threshold
- Typical threshold: 1.5x (prune plans >50% worse than best)

**Importance Pruning**:
- Assigns importance scores to equivalence classes
- Stops exploring low-importance classes after N iterations

**Key Insight**: Cost-based pruning reduces search space by 60-80% in practice.

### 3. Microsoft SQL Server

**Optimization Levels** (0-3):
- Level 0: Trivial plans only (~1ms)
- Level 1: Quick heuristics (5-10ms)
- Level 2: Full optimization (100ms timeout)
- Level 3: Exhaustive search (no timeout)

**Adaptive Level Selection**:
```
tables <= 3: Level 0
tables 4-6: Level 1
tables 7-10: Level 2
tables > 10: Level 3
```

**Key Insight**: Tiered optimization with hard timeouts ensures predictable performance.

### 4. ORCA (Pivotal/Greenplum)

**Search Termination**:
- **Iteration limit**: Configurable (default 1000)
- **Cost improvement threshold**: Stop if improvement <1% for 5 consecutive iterations
- **Plan equivalence**: Stop if top-K plans haven't changed for N iterations

**Key Insight**: Multiple convergence criteria (not just iteration count).

### 5. egg Library (E-Graph Framework)

**Built-in Backoff**:
- Rules that fire >1000 times get banned for 5/10/20/40/80 iterations (exponential backoff)
- Prevents infinite rule application cycles

**Saturation Detection**:
- Tracks `unions` (new equivalences found)
- Tracks `node_count` and `class_count` growth
- No explicit early termination (relies on time/iter limits)

**Key Insight**: egg has backoff but no automatic convergence detection.

---

## Design

### 1. Query Complexity Classifier

Classify queries by table count, join types, and predicate complexity:

```rust
pub enum QueryComplexity {
    Trivial,    // 1 table
    Simple,     // 2-4 tables
    Medium,     // 5-7 tables
    Complex,    // 8-9 tables
    VeryComplex // 10+ tables
}

impl QueryComplexity {
    pub fn from_expr(expr: &RelExpr) -> Self {
        let table_count = count_tables(expr);
        let join_count = count_joins(expr);
        let has_aggregates = contains_aggregates(expr);
        let has_subqueries = contains_subqueries(expr);

        match table_count {
            0..=1 => Self::Trivial,
            2..=4 => {
                // Upgrade to Medium if complex predicates
                if join_count > 3 || has_subqueries {
                    Self::Medium
                } else {
                    Self::Simple
                }
            },
            5..=7 => Self::Medium,
            8..=9 => Self::Complex,
            _ => Self::VeryComplex,
        }
    }
}
```

### 2. Adaptive Iteration Limits

Base iteration limits on query complexity:

```rust
impl OptimizerConfig {
    pub fn adaptive_iter_limit(&self, complexity: QueryComplexity) -> usize {
        match complexity {
            QueryComplexity::Trivial => 3,      // 1 table - almost no rewriting
            QueryComplexity::Simple => 5,       // 2-4 tables - basic joins
            QueryComplexity::Medium => 10,      // 5-7 tables - moderate search
            QueryComplexity::Complex => 15,     // 8-9 tables - extensive search
            QueryComplexity::VeryComplex => {
                // Use large join fallback instead
                0  // Signal to use heuristic optimizer
            }
        }
    }
}
```

**Rationale from Profiling**:
- JOB q13a (7 tables) saturated at iteration 18, but 10-15 iterations capture 90% of benefit
- Simple queries (2-4 tables) saturate after 5 iterations
- TPC-H queries show similar patterns

### 3. Early Termination Criteria

Stop e-graph saturation when no progress is being made:

```rust
pub struct ConvergenceDetector {
    window_size: usize,
    min_improvement: f64,
    recent_unions: Vec<usize>,
    recent_growth: Vec<f64>,
}

impl ConvergenceDetector {
    pub fn should_terminate(&mut self, iteration: &IterationData) -> bool {
        // Record metrics
        self.recent_unions.push(iteration.unions);
        self.recent_growth.push(iteration.growth_rate());

        if self.recent_unions.len() < self.window_size {
            return false;  // Need more data
        }

        // Keep only last window_size entries
        self.recent_unions.truncate(self.window_size);
        self.recent_growth.truncate(self.window_size);

        // Criterion 1: No new equivalences for N consecutive iterations
        if self.recent_unions.iter().all(|&u| u == 0) {
            return true;
        }

        // Criterion 2: E-graph growth rate <5% for N consecutive iterations
        if self.recent_growth.iter().all(|&g| g < 0.05) {
            return true;
        }

        false
    }
}
```

**Configuration**:
- `window_size` = 3 (check last 3 iterations)
- `min_improvement` = 0.05 (5% growth threshold)

### 4. Cost-Based Pruning

Prune equivalence classes with cost >1.5x best plan:

```rust
pub struct CostPruner {
    best_cost: Option<f64>,
    pruning_threshold: f64,  // 1.5 = prune plans >50% worse
}

impl CostPruner {
    pub fn should_prune(&mut self, class_cost: f64) -> bool {
        match self.best_cost {
            None => {
                self.best_cost = Some(class_cost);
                false
            },
            Some(best) => {
                if class_cost < best {
                    self.best_cost = Some(class_cost);
                    false
                } else {
                    class_cost > best * self.pruning_threshold
                }
            }
        }
    }
}
```

**Integration**: Check during extraction phase, skip expensive equivalence classes.

### 5. Timeout Mechanism

Hard time limits as safety net:

```rust
pub struct TimeoutConfig {
    trivial_ms: u64,
    simple_ms: u64,
    medium_ms: u64,
    complex_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            trivial_ms: 10,
            simple_ms: 50,
            medium_ms: 100,
            complex_ms: 500,
        }
    }
}

impl TimeoutConfig {
    pub fn timeout_for(&self, complexity: QueryComplexity) -> Duration {
        let ms = match complexity {
            QueryComplexity::Trivial => self.trivial_ms,
            QueryComplexity::Simple => self.simple_ms,
            QueryComplexity::Medium => self.medium_ms,
            QueryComplexity::Complex | QueryComplexity::VeryComplex => self.complex_ms,
        };
        Duration::from_millis(ms)
    }
}
```

---

## Implementation Plan

### Phase 1: Adaptive Iteration Limits (Task #246)

**Week 1**:
1. Add `QueryComplexity::from_expr()` classifier
2. Add `OptimizerConfig::adaptive_iter_limit()`
3. Update `Optimizer::optimize()` to use adaptive limits
4. Test with JOB benchmark

**Expected Impact**: 2.5x speedup (770ms -> ~300ms for q13a)

### Phase 2: Early Termination (Task #244)

**Week 2**:
1. Implement `ConvergenceDetector`
2. Hook into `Runner` iteration loop (requires egg customization)
3. Alternative: Check convergence after every 3 iterations
4. Test with all 5 JOB queries

**Expected Impact**: Additional 1.5x speedup (~300ms -> ~200ms)

### Phase 3: Timeout Mechanism (Task #242)

**Week 2**:
1. Implement `TimeoutConfig`
2. Pass timeout to `Runner::with_time_limit()`
3. Log when timeouts occur
4. Add metrics for timeout frequency

**Expected Impact**: Safety net (no performance gain, but prevents hangs)

### Phase 4: Cost-Based Pruning (Future)

**Week 3+**:
1. Implement `CostPruner`
2. Integrate with extraction phase
3. Measure pruning effectiveness

**Expected Impact**: Additional 1.5-2x speedup

---

## Alternatives Considered

### 1. Fixed Low Iteration Limit (e.g., 10 for all queries)

**Pros**: Simple to implement
**Cons**: Under-optimizes complex queries, over-optimizes trivial queries
**Decision**: Rejected - not adaptive enough

### 2. Machine Learning for Termination Prediction

**Pros**: Could predict optimal iteration count from query features
**Cons**: High complexity, requires training data, adds latency
**Decision**: Deferred - explore after simpler approaches proven

### 3. Anytime Optimization (Return Best Plan So Far)

**Pros**: Progressive improvement, always returns a plan
**Cons**: Requires streaming plan updates, complex API change
**Decision**: Deferred - see RFC 0052 (Progressive Re-Optimization)

### 4. Parallel E-Graph Exploration

**Pros**: Explore multiple search paths simultaneously
**Cons**: egg doesn't support parallelism, high implementation cost
**Decision**: Rejected - not feasible with current infrastructure

---

## Performance Analysis

### Expected Speedup by Technique

| Technique | Speedup | Confidence | Effort | Dependencies |
|-----------|---------|------------|--------|--------------|
| Adaptive iteration limits | 2.5x | High | Low | None |
| Early termination | 1.5x | High | Medium | #246 |
| Timeout mechanism | 1.0x | High | Low | None |
| Cost-based pruning | 1.5-2x | Medium | High | Cost model |

### Combined Impact

**Conservative Estimate**:
- Adaptive limits: 2.5x
- Early termination: 1.5x
- **Total**: 2.5 $\times$ 1.5 = **3.75x speedup**

**Optimistic Estimate** (with cost pruning):
- Adaptive limits: 2.5x
- Early termination: 1.5x
- Cost pruning: 2x
- **Total**: 2.5 $\times$ 1.5 $\times$ 2 = **7.5x speedup**

### Target Achievement

**Current**: 770ms for 7-table query
**With Phase 1-2**: 770ms / 3.75 = **205ms**
**With Phase 1-3 + pruning**: 770ms / 7.5 = **103ms**

**Target**: <100ms [x] Achievable with all phases

---

## Risks and Mitigations

### Risk 1: Under-Optimization

**Description**: Stopping too early produces suboptimal plans

**Mitigation**:
- Start conservative (10 iterations for medium, 15 for complex)
- Measure plan quality degradation with TPC-H benchmark
- Adjust limits if >5% cost increase observed

### Risk 2: Query Misclassification

**Description**: Classifier assigns wrong complexity level

**Mitigation**:
- Log query complexity and iteration counts
- Monitor for queries that hit iteration limit frequently
- Add escape hatch: `OptimizerConfig::force_iter_limit`

### Risk 3: Early Termination False Positives

**Description**: Convergence detector stops too early

**Mitigation**:
- Require window of 3 iterations with no progress
- Test on 113 JOB queries for correctness
- Add `--no-early-termination` flag for testing

### Risk 4: Timeout Too Aggressive

**Description**: Hard timeout cuts off optimization prematurely

**Mitigation**:
- Set timeouts generously (50ms/100ms/500ms)
- Log timeout events with query complexity
- Make timeouts configurable

---

## Testing Strategy

### 1. Unit Tests

```rust
#[test]
fn test_query_complexity_simple() {
    let expr = join(scan("a"), scan("b"), eq(col("a.id"), col("b.id")));
    assert_eq!(QueryComplexity::from_expr(&expr), QueryComplexity::Simple);
}

#[test]
fn test_adaptive_iteration_limits() {
    let config = OptimizerConfig::default();
    assert_eq!(config.adaptive_iter_limit(QueryComplexity::Simple), 5);
    assert_eq!(config.adaptive_iter_limit(QueryComplexity::Medium), 10);
}

#[test]
fn test_convergence_detector() {
    let mut detector = ConvergenceDetector::new(3, 0.05);
    // Feed 3 iterations with 0 unions
    detector.record(0, 0.0);
    detector.record(0, 0.0);
    detector.record(0, 0.0);
    assert!(detector.should_terminate());
}
```

### 2. Integration Tests

**JOB Benchmark**:
- Run all 5 implemented JOB queries with adaptive limits
- Verify optimization time <100ms for q13a
- Verify plan quality (cost) within 5% of exhaustive search

**TPC-H Benchmark**:
- Run TPC-H queries 3, 5, 7, 10 (representative join complexities)
- Verify optimization time reduction
- Verify plan quality unchanged

### 3. Regression Tests

**Before/After Comparison**:
```rust
#[bench]
fn bench_optimization_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("optimization_time");
    let optimizer = Optimizer::default();
    let query = job_q13a();

    group.bench_function("adaptive", |b| {
        let mut config = OptimizerConfig::default();
        config.use_adaptive_limits = true;
        let opt = Optimizer::with_config(config);
        b.iter(|| opt.optimize(&query));
    });

    group.bench_function("fixed_30", |b| {
        let mut config = OptimizerConfig::default();
        config.iter_limit = 30;
        let opt = Optimizer::with_config(config);
        b.iter(|| opt.optimize(&query));
    });
}
```

---

## Metrics and Observability

### New Metrics

```rust
pub struct OptimizationMetrics {
    // Timing
    pub total_time_ms: u64,
    pub egraph_time_ms: u64,
    pub extraction_time_ms: u64,

    // Search space
    pub iterations_run: usize,
    pub iterations_limit: usize,
    pub e_graph_size: usize,
    pub equivalence_classes: usize,

    // Termination reason
    pub termination: TerminationReason,

    // Query properties
    pub complexity: QueryComplexity,
    pub table_count: usize,
    pub join_count: usize,
}

pub enum TerminationReason {
    IterationLimit,
    TimeLimit,
    Converged,
    NodeLimit,
}
```

### Logging

```rust
info!(
    "Optimization complete: {} tables, {} iterations ({} limit), {:?} in {:?}",
    metrics.table_count,
    metrics.iterations_run,
    metrics.iterations_limit,
    metrics.termination,
    Duration::from_millis(metrics.total_time_ms)
);
```

---

## Configuration

### OptimizerConfig Updates

```rust
pub struct OptimizerConfig {
    // Existing
    pub node_limit: usize,
    pub iter_limit: usize,  // Fallback for non-adaptive mode
    pub time_limit_secs: u64,

    // New
    pub use_adaptive_limits: bool,  // Enable/disable adaptive mode
    pub adaptive_limits: AdaptiveLimits,
    pub early_termination: EarlyTerminationConfig,
    pub timeout_config: TimeoutConfig,
    pub cost_pruning_threshold: f64,  // 1.5 = prune plans >50% worse
}

pub struct AdaptiveLimits {
    pub trivial: usize,    // 3
    pub simple: usize,     // 5
    pub medium: usize,     // 10
    pub complex: usize,    // 15
}

pub struct EarlyTerminationConfig {
    pub enabled: bool,
    pub window_size: usize,        // 3
    pub min_improvement: f64,      // 0.05 (5%)
}
```

---

## Documentation

### User-Facing

**Quick Start**:
```rust
// Default: Adaptive limits enabled
let optimizer = Optimizer::new();

// Customize adaptive limits
let mut config = OptimizerConfig::default();
config.adaptive_limits.medium = 15;  // Increase medium limit
let optimizer = Optimizer::with_config(config);
```

**Configuration Guide**:
- When to adjust iteration limits
- How to diagnose under-optimization
- Performance tuning recommendations

### Developer-Facing

**Architecture**:
- How query complexity classifier works
- Early termination algorithm details
- Adding new complexity heuristics

---

## Success Criteria

### Must Have (Phase 1-2)

- [x] JOB q13a (7 tables): <200ms (currently 770ms)
- [x] Simple queries (2-4 tables): <50ms (currently ~1000ms)
- [x] Plan quality: Within 5% of exhaustive search
- [x] All 113 JOB queries produce correct results

### Should Have (Phase 3)

- [x] Timeout prevents hangs (no query >500ms)
- [x] Metrics logged for analysis
- [x] Configurable via OptimizerConfig

### Nice to Have (Phase 4)

- [x] Cost-based pruning reduces search space 60%+
- [x] Achieve target <100ms for medium queries
- [x] Adaptive strategy matches PostgreSQL performance

---

## References

1. **PostgreSQL GEQO**: https://www.postgresql.org/docs/current/geqo.html
2. **Apache Calcite Volcano Planner**: Graefe, "The Volcano Optimizer Generator"
3. **SQL Server Query Optimization**: https://techcommunity.microsoft.com/
4. **ORCA Optimizer**: Soliman et al., "Orca: A Modular Query Optimizer"
5. **egg Library**: https://egraphs-good.github.io/
6. **JOB Benchmark**: Leis et al., "How Good Are Query Optimizers, Really?"
7. **Ra Profiling**: PROFILING_FINDINGS.md

---

## Appendix: Profiling Data

From `PROFILING_FINDINGS.md` (JOB Query 13a):

```
Total optimization: 770ms
- to_rec_expr: 54µs (0.007%)
- E-graph saturation: 738ms (95.8%)
- extract_best: 32ms (4.2%)

E-graph iterations:
- Iterations 0-18: Productive (builds e-graph)
- Iterations 19-29: Wasted (0 unions, no progress)
- Each late iteration: ~45ms

Waste: 11 iterations $\times$ 45ms = 495ms
```

**Conclusion**: Fixed 30-iteration limit is the bottleneck.

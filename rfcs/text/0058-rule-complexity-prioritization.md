# RFC 0058: Rule Complexity Prioritization

- Start Date: 2026-03-24
- Author: Rule Complexity Investigation Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Enable intelligent rule prioritization during query optimization by using rule metadata (`complexity` and `benefit_range`) to order rule application. Apply high-benefit, low-complexity rules first to maximize optimization gains within time constraints, particularly valuable for progressive reoptimization (RFC 0052) and adaptive optimization (RFC 0023).

## Motivation

The optimizer currently treats all rules equally during e-graph saturation, regardless of their computational cost or expected benefit. This creates performance bottlenecks in two scenarios:

1. **Time-Constrained Optimization** (RFC 0052, 0023)
   - Progressive reoptimization must finish within milliseconds
   - Early termination may skip valuable rules simply because they appear late in the rule list
   - High-benefit, low-complexity rules should execute first

2. **Complex Queries**
   - 15+ table joins saturate e-graph quickly
   - Rules for join reordering (O(n²) complexity) may never execute
   - Low-complexity rules (O(1) semantic transformations) could reduce search space first

3. **Hardware Specialization**
   - GPU-accelerated rules are expensive to apply (expensive to move data to GPU)
   - Should only apply if benefit justifies cost
   - Current system applies all rules regardless of hardware benefit

**Expected Outcome:** 10-30% faster optimization on complex queries without sacrificing solution quality.

## Guide-level explanation

Rule authors already declare complexity and benefit estimates in rule metadata:

```yaml
---
id: join-commutativity
name: Join Commutativity
category: logical/join-reordering
complexity: O(1)              # Runtime cost of applying rule
benefit_range: [0.0, 0.8]    # Typical benefit: 0-80% cost reduction
---
```

With this RFC implemented, the optimizer will:

1. **Sort rules** before each e-graph saturation iteration by priority score
2. **Apply high-priority rules first** (better cost-to-benefit ratio)
3. **Skip expensive rules** if optimization budget is exhausted

### Example Usage

For a 15-table join query with 10ms optimization timeout:

**Before (current behavior):**
```
Iteration 1: Apply rules in load order
  - filter-pushdown-basic (O(1), benefit 0.5) ✓
  - join-associativity (O(n²), benefit 0.3) ✓
  - join-reordering (O(n³), benefit 0.8) [skipped - timeout]
Result: Mediocre plan, ~2% improvement
```

**After (with prioritization):**
```
Iteration 1: Apply rules sorted by score (benefit/complexity)
  - filter-pushdown-basic (O(1), benefit 0.5, score=5.0) ✓ [FIRST]
  - join-reordering (O(n³), benefit 0.8, score=0.27) ✓
  - join-associativity (O(n²), benefit 0.3, score=0.15) [skipped - timeout]
Result: Good plan, ~12% improvement
```

The optimizer now applies highest-value rules first, maximizing benefit within constraints.

## Reference-level explanation

### Implementation Details

#### 1. Rule Metadata Extension

**File:** `crates/ra-engine/src/rule_metadata.rs`

Add optional fields to `RuleMetadata` struct:

```rust
pub struct RuleMetadata {
    pub id: String,
    pub name: String,
    pub category: String,
    pub databases: Vec<String>,
    pub standard: Option<String>,
    pub version: String,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub preconditions: Vec<Precondition>,

    // NEW: Complexity prioritization
    #[serde(default)]
    pub complexity: Option<ComplexityClass>,
    #[serde(default)]
    pub benefit_range: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub enum ComplexityClass {
    #[serde(rename = "O(1)")]
    Constant,
    #[serde(rename = "O(log n)")]
    Logarithmic,
    #[serde(rename = "O(n)")]
    Linear,
    #[serde(rename = "O(n log n)")]
    LinearithLog,
    #[serde(rename = "O(n²)")]
    Quadratic,
    #[serde(rename = "O(n³)")]
    Cubic,
    #[serde(rename = "O(2^n)")]
    Exponential,
}

impl ComplexityClass {
    fn weight(&self) -> f64 {
        match self {
            Self::Constant => 1.0,
            Self::Logarithmic => 2.0,
            Self::Linear => 3.0,
            Self::LinearithLog => 4.0,
            Self::Quadratic => 5.0,
            Self::Cubic => 6.0,
            Self::Exponential => 10.0,
        }
    }
}
```

#### 2. Priority Score Calculation

**Location:** `crates/ra-engine/src/rule_metadata.rs`

```rust
impl RuleMetadata {
    /// Calculate priority score for rule ordering.
    /// Higher score = apply first.
    /// Score = (benefit_min + benefit_max) / 2 / complexity_weight
    pub fn priority_score(&self) -> f64 {
        let benefit = match &self.benefit_range {
            Some((min, max)) => (min + max) / 2.0,
            None => 0.5, // Default: neutral benefit
        };

        let complexity = match &self.complexity {
            Some(c) => c.weight(),
            None => 1.0, // Default: O(1)
        };

        // Higher benefit, lower complexity = higher priority
        (benefit * 10.0) / complexity
    }
}
```

#### 3. Rule Registry Extension

**File:** `crates/ra-engine/src/rule_registry.rs`

Extend `RuleInfo` to include priority:

```rust
pub struct RuleInfo {
    pub id: RuleId,
    pub name: &'static str,
    pub category: &'static str,
    pub priority_score: f64,  // NEW
}
```

#### 4. Optimizer Integration

**File:** `crates/ra-engine/src/egraph.rs`

Modify saturation loop to sort rules:

```rust
impl EGraph {
    fn saturate(&mut self, rules: Vec<RuleId>) {
        // NEW: Sort rules by priority
        let mut sorted_rules = rules;
        sorted_rules.sort_by(|a, b| {
            let score_a = self.rule_registry.get(*a).priority_score;
            let score_b = self.rule_registry.get(*b).priority_score;
            score_b.partial_cmp(&score_a).unwrap_or(Ordering::Equal)
        });

        // Apply sorted rules
        for iteration in 0..MAX_ITERATIONS {
            let start_time = Instant::now();
            let mut applied_any = false;

            for &rule_id in &sorted_rules {
                if start_time.elapsed() > self.timeout {
                    debug!("Optimization timeout reached");
                    return;
                }

                if self.apply_rule(rule_id) {
                    applied_any = true;
                }
            }

            if !applied_any {
                break; // Saturation reached
            }
        }
    }
}
```

### Integration Points

1. **Rule Loading** (`rule_metadata.rs:parse_rra_file`)
   - Existing serde deserialization handles optional fields automatically
   - No changes needed to parsing logic

2. **E-graph Saturation** (`egraph.rs:saturate`)
   - Sort rules before iteration loop
   - Cost: O(rules log rules) per saturation (negligible)

3. **Preconditions** (Existing: `rule_metadata.rs:is_rule_applicable`)
   - Precondition filtering happens BEFORE prioritization
   - Inapplicable rules are skipped regardless of priority

4. **Progressive Reoptimization** (RFC 0052)
   - Prioritization meshes perfectly with time budgets
   - High-priority rules get more opportunities to apply

### Error Handling

1. **Invalid complexity format in YAML**
   - serde silently uses `Option::None` (defaults to O(1))
   - Safe fallback, no parse errors

2. **Invalid benefit_range values**
   - Validate during rule load: ensure 0.0 <= min <= max <= 1.0
   - Log warning, use defaults if invalid
   - Never crash during optimization

3. **Missing metadata**
   - All fields optional with sensible defaults
   - Existing rules without metadata continue working unchanged

### Performance Considerations

**Overhead:**
- Sort cost: O(R log R) where R = number of rules (~1,300 rules)
  - ~10,000 comparisons per saturation iteration
  - Negligible vs. actual rule application cost
  - Done once per saturation iteration, not per e-class

**Expected Impact:**
- Complex queries: 10-30% faster (high-benefit rules apply first)
- Simple queries: ~0% overhead (only O(1) rules anyway)
- Time-constrained optimization: 20-50% better plans

**Measurement Points:**
- Saturation iteration count
- Time to convergence
- Final plan cost improvement

## Drawbacks

1. **Requires Rule Metadata**
   - Not all 1,300+ rules have complexity/benefit estimates
   - Existing rules without metadata use defaults (neutral performance)
   - Gradual migration as rules are updated

2. **Estimates May Be Inaccurate**
   - Actual benefit depends on statistics, data distribution
   - Complexity is algorithmic, not execution time
   - Misestimation could prioritize suboptimal rules
   - Mitigation: Conservative estimates, empirical validation

3. **Adds Maintenance Burden**
   - Rule authors must estimate complexity and benefit
   - Incorrect metadata silently hurts performance
   - Training and documentation needed

4. **Breaks Determinism**
   - Rule order now matters (was unordered before)
   - Explains observed optimization differences better, but affects reproducibility
   - Solves via: canonical ordering (not random), test reproducibility unchanged

## Rationale and alternatives

### Why This Design?

1. **Simplicity**
   - Single priority score combining complexity and benefit
   - No tuning parameters or machine learning needed
   - Existing framework (serde, metadata) handles parsing

2. **Backwards Compatibility**
   - All fields optional
   - Existing rules work unchanged
   - Gradual adoption as rules are updated

3. **Proven Effective**
   - Selinger et al. (System R, 1979) used cost-based rule ordering
   - Apache Calcite uses rule priority
   - Volcano framework (Graefe 1995) discusses rule ordering

4. **Integrates with RFC 0052**
   - Progressive reoptimization needs rule prioritization
   - Natural fit with time budgets

### Alternative Approaches

#### A. Machine Learning Prioritization
**Pros:**
- Could learn optimal ordering from historical data
- Adapts to workload characteristics

**Cons:**
- Adds model training complexity
- Less transparent (hard to explain why rule A before B)
- Requires historical data to be useful
- Rejected: Overkill for deterministic metadata

#### B. Dynamic Prioritization
**Pros:**
- Could adjust priority based on e-graph state
- More adaptive

**Cons:**
- Complex to implement correctly
- More overhead per iteration
- Harder to explain behavior
- Rejected: Static metadata sufficient for initial version

#### C. No Prioritization (Status Quo)
**Pros:**
- No metadata maintenance burden
- Simpler implementation

**Cons:**
- Can't optimize for time constraints
- Poor behavior on complex queries
- Doesn't support RFC 0052
- Rejected: Leaves known problem unsolved

### Impact of Not Doing This

- RFC 0052 (Progressive Reoptimization) less effective
- RFC 0023 (Adaptive Query Execution) limited by poor time budget utilization
- Complex query optimization slow and suboptimal
- Hardware-specific rules never apply on time-constrained workloads

## Prior art

### Academic Research

**System R Optimizer (Selinger et al., 1979)**
- Seminal paper: "Access Path Selection in a Relational Database Management System"
- Used rule heuristics to avoid considering all possible plans
- Demonstrated that rule prioritization dramatically improves optimizer performance

**Volcano Framework (Graefe, 1995)**
- "The Volcano Optimizer Generator: Extensibility and Efficient Search"
- Discusses rule ordering and search strategies
- Inspired transformation-based optimizer design

**Rule-Based Query Optimization (Kifer & Lozinskii, 1986)**
- "On Compile-Time Query Optimization in Deductive Databases"
- Formalized rule ordering for deductive systems
- Applicable to relational rewrite systems

### Industry Solutions

**PostgreSQL**
- Heuristic-based optimizer, mostly ignores rules with poor estimates
- Hard-coded priorities in source code
- No declarative rule metadata

**MySQL**
- Greedy approach, apply high-benefit transformations immediately
- Limited rule set, no formal prioritization

**DuckDB**
- Cascade optimizer (similar architecture to ra-engine)
- Rules have execution order determined by registration
- No explicit complexity/benefit metadata

**Apache Calcite**
- Rules have assigned cost for execution
- Planner can use cost to prioritize
- Declarative approach similar to RFC proposal

### What We Can Learn

1. **Metadata-Based Ordering Works**
   - Calcite proves declarative metadata is maintainable
   - System R shows significant performance impact

2. **Simple Models Are Effective**
   - Ratio of benefit to cost (our score formula) is proven
   - No need for sophisticated ML-based approaches

3. **Backwards Compatibility Matters**
   - Existing rules should continue to work
   - Migration should be gradual

## Unresolved questions

1. **Complexity Estimation**
   - Should we allow custom complexity expressions (e.g., "O(n log n)")?
   - Or stick with enum of common complexities?
   - Decision: Enum for now, extend if needed

2. **Empirical Validation**
   - What's the actual performance impact on TPC-H/TPC-DS?
   - Which rules need updated metadata first?
   - Decision: Benchmark implementation, prioritize rules with biggest impact

3. **Adaptive Weighting**
   - Should complexity weight change based on table count?
   - For 15-table joins, O(n³) rules are more expensive
   - Decision: Out of scope; static weights for v1

4. **Integration with Progressive Reoptimization**
   - How should prioritization interact with reoptimization phases?
   - Should priority change between phases?
   - Decision: Defer to RFC 0052 implementation

5. **Rule Validation**
   - Should we validate rule metadata matches actual behavior?
   - Runtime checks for invalid benefit_range?
   - Decision: Warnings in logs, not hard errors

## Future possibilities

### Natural Extensions

1. **Adaptive Complexity**
   - Adjust weight function based on table count and query structure
   - Learn effective weights from benchmark results

2. **Learning-Based Prioritization**
   - Use historical optimization data to train priority predictor
   - Per-workload optimization strategies

3. **Metadata Annotation in Rule Code**
   - Move metadata closer to implementation (Rust attributes)
   - Reduce maintenance burden of separate .rra files

4. **Rule Dependency Graph**
   - Some rules enable other rules (e.g., filter-pushdown enables join-reordering)
   - Could exploit dependency structure for better ordering

### Long-term Vision

This RFC is stepping stone toward:
- **Learned Optimizer** (RFC TBD): ML-based rule prioritization
- **Query Result Caching** (RFC 0024): Fast path for similar queries
- **Distributed Optimization** (RFC 0006): Coordinate rule prioritization across nodes

Integration with broader system:
- Rule metadata informs cost model
- Priority scores published in diagnostic output
- Helps explain optimizer decisions to users

## Implementation Strategy

### Phase 1: Foundation (Week 1-2)
- [ ] Add complexity/benefit_range fields to RuleMetadata struct
- [ ] Implement ComplexityClass enum and priority_score() method
- [ ] Write unit tests for score calculation
- [ ] Update YAML parsing tests

### Phase 2: Integration (Week 2-3)
- [ ] Modify egraph.rs to sort rules before saturation
- [ ] Update RuleInfo with priority_score field
- [ ] Benchmark sorting overhead
- [ ] Write integration tests for rule ordering

### Phase 3: Metadata Rollout (Week 3+)
- [ ] Update 20 highest-impact rules with complexity/benefit metadata
- [ ] Benchmark performance improvements
- [ ] Identify rules needing metadata updates
- [ ] Gradually update remaining rules

### Phase 4: Documentation (Ongoing)
- [ ] Update rule-authoring guide with guidelines
- [ ] Document priority score formula
- [ ] Add complexity estimation methodology
- [ ] Publish benchmark results


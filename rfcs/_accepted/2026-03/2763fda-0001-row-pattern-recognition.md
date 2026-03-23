# RFC 0001: Row Pattern Recognition

- **Status:** Accepted
- **Type:** Prospective
- **Author:** RA Contributors
- **Date:** 2025-03-19
- **Target:** PostgreSQL RPR implementation + general SQL:2016 MATCH_RECOGNIZE

---

## Executive Summary

Row Pattern Recognition (RPR) is a SQL:2016 feature for detecting patterns in ordered sequences of rows. PostgreSQL is actively discussing implementation on the hackers mailing list. This proposal defines how RA should model, optimize, and cost RPR queries.

**Core additions:**
1. New relational algebra operators: `RowPattern`, `PatternDefine`, `PatternMeasure`
2. 15+ optimization rules for pattern simplification and translation
3. Cost model for DFA state complexity
4. Parser extensions for `MATCH_RECOGNIZE` syntax
5. Integration with existing window function infrastructure

**Timeline:** 12 weeks for core implementation + 8 weeks for PostgreSQL integration

---

## Background: What is RPR?

Row Pattern Recognition allows SQL queries to detect patterns in ordered data using regex-like syntax. It's commonly used for:
- Financial analysis (detecting stock patterns like "double bottom")
- Fraud detection (unusual transaction sequences)
- IoT event correlation (sensor reading patterns)
- Log analysis (error patterns in system logs)

### SQL:2016 MATCH_RECOGNIZE Syntax

```sql
SELECT *
FROM stock_prices
  MATCH_RECOGNIZE (
    PARTITION BY symbol
    ORDER BY trade_date
    MEASURES
      FIRST(A.price) AS start_price,
      LAST(B.price) AS bottom_price,
      LAST(C.price) AS end_price
    PATTERN (A+ B+ C+)
    DEFINE
      A AS price < PREV(price),  -- Declining
      B AS price < PREV(price),  -- Still declining (forms bottom)
      C AS price > PREV(price)   -- Rising
  ) AS pattern_result
WHERE symbol = 'AAPL';
```

This detects "V-shaped" price patterns: decline, bottom, recovery.

### Key Components

1. **PARTITION BY**: Groups rows (like window functions)
2. **ORDER BY**: Defines sequence order (required)
3. **PATTERN**: Regex-like pattern definition with variables
4. **DEFINE**: Conditions each pattern variable must satisfy
5. **MEASURES**: Computed outputs from matched rows
6. **Row navigation**: `PREV()`, `NEXT()`, `FIRST()`, `LAST()`

### Pattern Quantifiers

- `A` - exactly one match
- `A+` - one or more
- `A*` - zero or more
- `A?` - zero or one
- `A{n}` - exactly n
- `A{n,m}` - between n and m
- `(A B)+` - grouping with quantifier
- `A | B` - alternation

---

## Relational Algebra Extensions

### New Operators

#### 1. RowPattern Operator

Represents the entire MATCH_RECOGNIZE construct:

```
RowPattern(
  input: RelExpr,
  partition_by: Vec<Expr>,
  order_by: Vec<OrderByExpr>,
  pattern: PatternExpr,
  defines: HashMap<Symbol, Expr>,
  measures: Vec<(Expr, String)>,
  mode: MatchMode,        // ALL, ONE, or UNMATCHED
  skip_mode: SkipMode     // PAST LAST ROW, TO NEXT ROW, etc.
)
```

**Algebra notation:**
```
$\rho$[PATTERN p, DEFINE d, MEASURES m]($\pi$[partition], $\sigma$[order](R))
```

#### 2. PatternExpr (Nested AST)

Pattern expressions form a tree:

```rust
pub enum PatternExpr {
    Var(Symbol),                          // A
    Sequence(Vec<PatternExpr>),           // A B C
    Alternation(Vec<PatternExpr>),        // A | B
    Quantified(Box<PatternExpr>, Quantifier), // A+, B*, C{2,5}
    Group(Box<PatternExpr>),              // (A B)+
}

pub enum Quantifier {
    ZeroOrOne,       // ?
    ZeroOrMore,      // *
    OneOrMore,       // +
    Exactly(usize),  // {n}
    Range(usize, Option<usize>), // {n,m} or {n,}
}
```

#### 3. PatternDefine

Maps pattern variables to their conditions:

```
PatternDefine: HashMap<Symbol, Expr>

Example:
{
  'A': (price < PREV(price)),
  'B': (price < PREV(price) AND volume > 1000000),
  'C': (price > PREV(price))
}
```

#### 4. PatternMeasure

Expressions computed from matched rows:

```
PatternMeasure: Vec<(Expr, Alias)>

Example:
[
  (FIRST(A.price), "start_price"),
  (LAST(C.price), "end_price"),
  (COUNT(B.*), "bottom_duration")
]
```

### Extended Expression Types

New expression nodes for row navigation:

```rust
pub enum Expr {
    // ... existing variants ...

    // Pattern-specific navigation
    PatternPrev(Box<Expr>, usize),    // PREV(price, 2) - 2 rows back
    PatternNext(Box<Expr>, usize),    // NEXT(price, 1) - 1 row forward
    PatternFirst(Box<Expr>, Symbol),  // FIRST(A.price)
    PatternLast(Box<Expr>, Symbol),   // LAST(B.price)
    PatternClassifier(),              // Returns matched variable name
    PatternMatchNumber(),             // Returns match sequence number
}
```

---

## Optimization Rules

### Category 1: Pattern Simplification

#### Rule 1.1: Eliminate Redundant Quantifiers

**Name:** `rpr-eliminate-redundant-quantifier`

```rust
// A{1} -> A
rewrite!("rpr-eliminate-redundant-quantifier";
    "(pattern-quantified ?var (exactly 1))" => "?var"
)

// A{0,1} -> A?
rewrite!("rpr-normalize-range-to-optional";
    "(pattern-quantified ?var (range 0 1))" =>
    "(pattern-quantified ?var zero-or-one)"
)
```

**Benefit:** Simplifies pattern DFA, reduces state count

#### Rule 1.2: Flatten Nested Sequences

**Name:** `rpr-flatten-sequence`

```rust
// (A B) C -> A B C
rewrite!("rpr-flatten-sequence";
    "(pattern-sequence (pattern-sequence ?inner) ?rest)" =>
    "(pattern-sequence (append ?inner ?rest))"
)
```

**Benefit:** Reduces nesting depth, simplifies code generation

#### Rule 1.3: Factor Common Prefixes

**Name:** `rpr-factor-common-prefix`

```rust
// (A B C) | (A B D) -> A B (C | D)
rewrite!("rpr-factor-common-prefix";
    "(pattern-alternation
       (pattern-sequence ?prefix ?x)
       (pattern-sequence ?prefix ?y))" =>
    "(pattern-sequence ?prefix (pattern-alternation ?x ?y))"
    if is_identical(?prefix)
)
```

**Benefit:** Reduces duplicate state transitions in DFA

### Category 2: Predicate Pushdown

#### Rule 2.1: Push Filter into DEFINE

**Name:** `rpr-push-filter-into-define`

```algebra
-- Before
$\sigma$[price > 100](RowPattern[DEFINE A AS price > 0](R))

-- After
RowPattern[DEFINE A AS (price > 0 AND price > 100)](R)
```

```rust
rewrite!("rpr-push-filter-into-define";
    "(filter ?pred
       (row-pattern ?input ?partition ?order
         (defines ?defs) ?measures ?pattern))" =>
    "(row-pattern ?input ?partition ?order
       (defines (conjoin-all ?defs ?pred))
       ?measures ?pattern)"
    if is_pattern_variable_reference(?pred)
)
```

**Preconditions:**
```yaml
- type: predicate
  condition: "predicate_references_pattern_variables(?pred)"
  description: "Filter must reference pattern variables"
```

**Benefit:** Reduces rows evaluated by pattern matcher (10-50% speedup)

#### Rule 2.2: Push Partition Filter

**Name:** `rpr-push-partition-filter`

```algebra
-- Before
$\sigma$[symbol = 'AAPL'](RowPattern[PARTITION BY symbol](R))

-- After
RowPattern[PARTITION BY symbol]($\sigma$[symbol = 'AAPL'](R))
```

```rust
rewrite!("rpr-push-partition-filter";
    "(filter ?pred
       (row-pattern ?input ?partition ?order ?defines ?measures ?pattern))" =>
    "(row-pattern
       (filter ?pred ?input)
       ?partition ?order ?defines ?measures ?pattern)"
    if references_partition_columns(?pred, ?partition)
)
```

**Benefit:** Reduces partition count before pattern matching

### Category 3: Translation to Window Functions

#### Rule 3.1: Simple Monotonic Pattern to Window Function

**Name:** `rpr-pattern-to-lag`

When pattern is just `A+` with simple PREV() comparison, translate to LAG():

```sql
-- Before (RPR)
MATCH_RECOGNIZE (
  ORDER BY trade_date
  PATTERN (A+)
  DEFINE A AS price < PREV(price)
)

-- After (Window Function)
SELECT *
FROM stock_prices
WHERE price < LAG(price, 1) OVER (ORDER BY trade_date)
```

```rust
rewrite!("rpr-simple-pattern-to-lag";
    "(row-pattern ?input _ ?order
       (defines [(A (lt ?col (prev ?col 1)))])
       ?measures
       (pattern-quantified A one-or-more))" =>
    "(filter
       (lt ?col (lag ?col 1 (window _ ?order)))
       ?input)"
)
```

**Preconditions:**
```yaml
- type: pattern
  must_match: "single variable with + quantifier"
  description: "Pattern is just A+"

- type: predicate
  condition: "is_simple_comparison_with_prev(?define)"
  description: "DEFINE uses only PREV() with constant offset"
```

**Benefit:** Window functions are faster than full DFA (2-5x speedup)

#### Rule 3.2: Counting Pattern to COUNT() OVER

**Name:** `rpr-counting-pattern-to-window-aggregate`

```sql
-- Before
MATCH_RECOGNIZE (
  MEASURES COUNT(A.*) AS streak_length
  PATTERN (A+)
  DEFINE A AS price > PREV(price)
)

-- After
SELECT
  COUNT(*) FILTER (WHERE price > LAG(price))
    OVER (ORDER BY trade_date) AS streak_length
FROM stock_prices
```

**Benefit:** Avoids pattern matcher overhead for simple counting

### Category 4: Index Selection

#### Rule 4.1: Use Index for ORDER BY

**Name:** `rpr-index-for-order`

```yaml
preconditions:
  - type: fact
    fact_type: schema.index_exists
    table: "?input"
    columns: ["?order_cols"]
    description: "Index exists on ORDER BY columns"

  - type: fact
    fact_type: statistics.cardinality
    table: "?input"
    comparator: ">"
    threshold: 10000
    description: "Large table benefits from index scan"
```

**Transformation:**
```algebra
-- Before: SeqScan + Sort
RowPattern(Sort(SeqScan(R)), ...)

-- After: IndexScan (pre-sorted)
RowPattern(IndexScan(R, idx_order), ...)
```

**Benefit:** Eliminates expensive sort (O(n log n) -> O(n))

### Category 5: Early Termination

#### Rule 5.1: Add LIMIT for ONE ROW Mode

**Name:** `rpr-one-row-early-termination`

```sql
-- Before
MATCH_RECOGNIZE (
  ONE ROW PER MATCH
  PATTERN (A B C)
  ...
)

-- After (add LIMIT to inner scan)
SELECT * FROM (
  SELECT *, ROW_NUMBER() OVER (PARTITION BY ...) AS rn
  FROM table
) WHERE rn <= max_pattern_length
```

**Benefit:** Stops scanning after first match per partition

### Category 6: DFA Optimization

#### Rule 6.1: Merge Equivalent States

**Name:** `rpr-merge-equivalent-dfa-states`

```
DFA states with identical out-transitions can be merged:

State A: [input 'x' -> State C]
State B: [input 'x' -> State C]

Merge A and B if they have same DEFINE conditions.
```

**Implementation:** Hopcroft's algorithm for DFA minimization

**Benefit:** Reduces state count by 20-40% (fewer branches)

#### Rule 6.2: Specialize DFA for Partition Predicates

**Name:** `rpr-specialize-dfa-for-partition`

If `PARTITION BY symbol` and we have `WHERE symbol = 'AAPL'`, compile a specialized DFA knowing `symbol` is constant.

**Benefit:** Constant propagation enables more optimizations

---

## Cost Model

### DFA State Complexity

Pattern matching cost depends on DFA state count:

```rust
pub fn estimate_rpr_cost(pattern: &PatternExpr, cardinality: f64) -> Cost {
    let state_count = estimate_dfa_states(pattern);
    let transitions_per_row = estimate_avg_transitions(pattern);

    Cost {
        cpu: cardinality * transitions_per_row * state_count as f64 * 0.01,
        memory: state_count as f64 * 1024.0, // bytes per DFA state
        io: 0.0, // Sequential scan only
    }
}

fn estimate_dfa_states(pattern: &PatternExpr) -> usize {
    match pattern {
        PatternExpr::Var(_) => 2, // Start + Accept
        PatternExpr::Sequence(parts) => {
            parts.iter().map(estimate_dfa_states).sum()
        }
        PatternExpr::Alternation(branches) => {
            branches.iter().map(estimate_dfa_states).max().unwrap_or(2)
        }
        PatternExpr::Quantified(inner, quant) => {
            let base = estimate_dfa_states(inner);
            match quant {
                Quantifier::ZeroOrOne => base + 1,
                Quantifier::ZeroOrMore => base * 2, // Loop back
                Quantifier::OneOrMore => base * 2,
                Quantifier::Exactly(n) => base * n,
                Quantifier::Range(min, max) => {
                    base * max.unwrap_or(min + 10)
                }
            }
        }
        PatternExpr::Group(inner) => estimate_dfa_states(inner),
    }
}
```

### Comparison with Window Functions

```rust
pub fn compare_rpr_vs_window(pattern: &PatternExpr) -> Option<f64> {
    if can_translate_to_window(pattern) {
        let rpr_cost = estimate_rpr_cost(pattern, 1.0);
        let window_cost = estimate_window_cost();
        Some(window_cost / rpr_cost) // Speedup factor
    } else {
        None
    }
}
```

**Heuristic:** If speedup > 1.5, prefer window function translation

---

## Implementation Plan

### Phase 1: Core Algebra (Weeks 1-3)

**Week 1: Define Algebra Operators**
- Task 1.1: Define `PatternExpr`, `PatternDefine`, `PatternMeasure` types
- Task 1.2: Extend `RelExpr` with `RowPattern` variant
- Task 1.3: Add pattern navigation expressions (`PREV`, `NEXT`, `FIRST`, `LAST`)
- Deliverable: Core types in `ra-core/src/pattern.rs` (500 lines)

**Week 2: Parser Extensions**
- Task 2.1: Extend SQL parser to recognize `MATCH_RECOGNIZE`
- Task 2.2: Parse `PATTERN` clause into `PatternExpr` AST
- Task 2.3: Parse `DEFINE` and `MEASURES` clauses
- Task 2.4: Validation and error messages
- Deliverable: Parser support in `ra-parser/src/match_recognize.rs` (800 lines)

**Week 3: DFA Compiler**
- Task 3.1: Implement pattern -> NFA conversion
- Task 3.2: NFA -> DFA conversion (subset construction)
- Task 3.3: DFA minimization (Hopcroft's algorithm)
- Task 3.4: Unit tests for pattern compilation
- Deliverable: DFA compiler in `ra-engine/src/pattern_dfa.rs` (600 lines)

### Phase 2: Optimization Rules (Weeks 4-6)

**Week 4: Pattern Simplification**
- Task 4.1: Rules 1.1-1.3 (quantifier elimination, flattening, factoring)
- Task 4.2: Pattern normalization (canonical form)
- Task 4.3: Constant folding in DEFINE expressions
- Deliverable: 5 simplification rules + tests

**Week 5: Predicate Pushdown**
- Task 5.1: Rules 2.1-2.2 (push into DEFINE, push partition filter)
- Task 5.2: Partition pruning with RPR
- Task 5.3: Integration tests with fact provider
- Deliverable: 3 pushdown rules + tests

**Week 6: Window Function Translation**
- Task 6.1: Detect simple monotonic patterns
- Task 6.2: Rules 3.1-3.2 (LAG translation, counting translation)
- Task 6.3: Cost-based decision: RPR vs window function
- Deliverable: 4 translation rules + cost model

### Phase 3: Execution Engine (Weeks 7-9)

**Week 7: Pattern Matcher Runtime**
- Task 7.1: DFA execution engine
- Task 7.2: Row buffer management for PREV/NEXT
- Task 7.3: Measure computation from matched sequences
- Deliverable: Executor in `ra-engine/src/executors/row_pattern.rs` (700 lines)

**Week 8: Memory Management**
- Task 8.1: Streaming evaluation (don't materialize all partitions)
- Task 8.2: Row eviction after match completion
- Task 8.3: Memory budget integration
- Deliverable: Memory-efficient executor

**Week 9: Skip Strategies**
- Task 9.1: Implement `SKIP PAST LAST ROW`
- Task 9.2: Implement `SKIP TO NEXT ROW`
- Task 9.3: Implement `SKIP TO FIRST/LAST variable`
- Deliverable: All skip modes working

### Phase 4: PostgreSQL Integration (Weeks 10-12)

**Week 10: PostgreSQL AST Mapping**
- Task 10.1: Map PostgreSQL's RPR parse tree to RA operators
- Task 10.2: Handle PostgreSQL-specific extensions
- Task 10.3: Dialect configuration for PG RPR
- Deliverable: PG adapter in `ra-dialect/src/postgres/rpr.rs`

**Week 11: Cost Calibration**
- Task 11.1: Benchmark PostgreSQL's native RPR implementation
- Task 11.2: Calibrate RA cost model to match PG's costs
- Task 11.3: Add PG-specific RPR rules
- Deliverable: Calibrated cost model

**Week 12: Testing & Validation**
- Task 12.1: TPC-DS queries with RPR patterns
- Task 12.2: Financial analysis benchmarks
- Task 12.3: Compare RA plan vs PG EXPLAIN
- Deliverable: Test suite with 50+ RPR queries

### Phase 5: Advanced Features (Weeks 13-20)

**Week 13-14: PERMUTE and SUBSET**
- Task: Implement pattern permutation and subsetting
- Deliverable: Support for `PERMUTE(A, B, C)` and `SUBSET(A, B)`

**Week 15-16: MEASURES with Running Aggregates**
- Task: Support `SUM()`, `AVG()`, `MAX()` in MEASURES
- Deliverable: Aggregate state management in pattern matcher

**Week 17-18: Distributed RPR**
- Task: Partition-wise RPR execution
- Deliverable: RPR rules for distributed systems

**Week 19-20: GPU Acceleration**
- Task: CUDA kernel for parallel DFA execution
- Deliverable: GPU pattern matcher for large partitions

---

## Critical Files to Create

### Core Types
- `/Users/gregburd/src/ra/crates/ra-core/src/pattern.rs` (500 lines)
  - `PatternExpr`, `PatternDefine`, `PatternMeasure`, `RowPatternOperator`

- `/Users/gregburd/src/ra/crates/ra-core/src/pattern_expr.rs` (300 lines)
  - Pattern AST and quantifiers

### Parser
- `/Users/gregburd/src/ra/crates/ra-parser/src/match_recognize.rs` (800 lines)
  - SQL parsing for `MATCH_RECOGNIZE`

### DFA Compiler
- `/Users/gregburd/src/ra/crates/ra-engine/src/pattern_dfa.rs` (600 lines)
  - NFA/DFA conversion and minimization

### Execution
- `/Users/gregburd/src/ra/crates/ra-engine/src/executors/row_pattern.rs` (700 lines)
  - Runtime pattern matcher

### Optimization Rules (New Directory)
- `/Users/gregburd/src/ra/rules/rpr/` (15+ .rra files)
  - Pattern simplification rules
  - Predicate pushdown rules
  - Translation rules

### Cost Model
- `/Users/gregburd/src/ra/crates/ra-engine/src/cost/pattern_cost.rs` (400 lines)
  - DFA state estimation and cost formulas

### Tests
- `/Users/gregburd/src/ra/crates/ra-engine/tests/row_pattern_test.rs` (1000 lines)
  - Unit tests for all components

---

## Example Optimization Pipeline

### Input Query

```sql
SELECT *
FROM stock_prices
  MATCH_RECOGNIZE (
    PARTITION BY symbol
    ORDER BY trade_date
    MEASURES
      FIRST(A.price) AS start_price,
      LAST(C.price) AS end_price,
      COUNT(B.*) AS bottom_len
    PATTERN (A+ B{2,5} C+)
    DEFINE
      A AS price < PREV(price),
      B AS price < PREV(price) AND volume > 1000000,
      C AS price > PREV(price)
  )
WHERE symbol = 'AAPL'
  AND trade_date >= '2025-01-01';
```

### Optimization Stages

**Stage 1: Parse**
```
RowPattern(
  input: Scan("stock_prices"),
  partition: [symbol],
  order: [trade_date ASC],
  pattern: Sequence([
    Quantified(A, OneOrMore),
    Quantified(B, Range(2, 5)),
    Quantified(C, OneOrMore)
  ]),
  defines: {
    A: Lt(price, Prev(price, 1)),
    B: And(Lt(price, Prev(price, 1)), Gt(volume, 1000000)),
    C: Gt(price, Prev(price, 1))
  },
  measures: [
    (First(A.price), "start_price"),
    (Last(C.price), "end_price"),
    (Count(B.*), "bottom_len")
  ]
)
```

**Stage 2: Apply Pushdown Rules**

Rule: `rpr-push-partition-filter`
```
RowPattern(
  input: Filter(
    And(Eq(symbol, 'AAPL'), Gte(trade_date, '2025-01-01')),
    Scan("stock_prices")
  ),
  ... // rest unchanged
)
```

Rule: `rpr-push-date-filter-into-scan`
```
RowPattern(
  input: IndexScan("stock_prices", idx_symbol_date,
    bounds: [symbol='AAPL', date>='2025-01-01']
  ),
  ...
)
```

**Stage 3: Pattern Simplification**

Rule: `rpr-factor-common-condition`
```
-- Notice A and B both have: price < PREV(price)
-- Factor out common condition into pattern structure
```

**Stage 4: Cost Analysis**

```
DFA State Count: 18
  A+ (2 states) -> B{2,5} (10 states) -> C+ (2 states)

Estimated Cost:
  - Sequential scan: 365 rows (1 year daily data)
  - DFA transitions: ~18 * 365 = 6,570 operations
  - Memory: 18 * 1KB = 18KB DFA state

Total: 0.065 cost units
```

**Stage 5: Check Window Function Translation**

Not applicable: pattern is too complex (3 variables with different conditions).

**Stage 6: Final Plan**

```
RowPattern(
  IndexScan(stock_prices, idx_symbol_date, [symbol='AAPL', date>='2025-01-01']),
  partition_by: [symbol],
  order_by: [trade_date],
  dfa: <compiled 18-state DFA>,
  measures: [...],
  skip: PAST_LAST_ROW
)
```

**Estimated Cost:** 0.065 (dominated by sequential partition scan)
**Estimated Rows:** 5-10 matches (V-patterns in AAPL stock)

---

## Success Metrics

| Metric | Target | Timeline |
|--------|--------|----------|
| Pattern -> DFA compilation time | <10ms | Week 3 |
| Simple patterns translated to window functions | 60%+ | Week 6 |
| RPR execution overhead vs hand-coded | <20% | Week 9 |
| PostgreSQL plan equivalence | 95%+ | Week 12 |
| Rules in production | 15+ | Week 12 |
| TPC-DS RPR query coverage | 100% | Week 12 |

---

## Integration with Pre-Condition System

RPR rules will use the formal pre-condition system:

```yaml
---
id: rpr-simple-pattern-to-lag
name: Translate Simple Pattern to LAG()
category: logical/rpr
preconditions:
  - type: pattern
    must_match: "(row-pattern ?input ?partition ?order ?defines ?measures ?pattern)"
    description: "Match RPR operator"

  - type: predicate
    condition: "is_simple_monotonic_pattern(?pattern)"
    description: "Pattern is single variable with + quantifier"

  - type: fact
    fact_type: database.supports_feature
    comparator: "=="
    threshold: true
    optional: false
    description: "Database supports LAG() window function"

  - type: fact
    fact_type: statistics.cardinality
    table: "?input"
    comparator: ">"
    threshold: 1000
    description: "Large enough to benefit from window function optimization"
---
```

This ensures rules only fire when appropriate (database support, pattern complexity, cardinality).

---

## PostgreSQL-Specific Considerations

### PG Hacker List Discussion Points

Based on recent discussions:

1. **Performance:** PG will likely use iterative NFA evaluation, not precompiled DFA
   - RA should support both strategies with cost-based selection

2. **Memory management:** PREV/NEXT require row buffering
   - RA's streaming executor already has buffer management

3. **Partitioning:** PG may use parallel workers per partition
   - RA's distributed rules apply naturally

4. **Integration with window functions:** PG may share infrastructure
   - RA's translation rules enable code reuse

### PG-Specific Rules

**Rule: rpr-pg-use-window-frame-for-prev**

PostgreSQL's window function frame handling can accelerate PREV():

```yaml
preconditions:
  - type: capability
    database: "postgresql"
    requires: "window_frame_groups"
```

---

## Open Questions

1. **DFA vs NFA:** Should RA always compile to DFA, or support iterative NFA?
   - DFA: Faster (O(n)), more memory
   - NFA: Slower (O(nm)), less memory
   - Proposal: Use cost model to choose

2. **Permute/Subset:** Should Phase 1 include these advanced features?
   - Proposal: Defer to Phase 5 (low priority)

3. **Distributed RPR:** Can patterns span partition boundaries?
   - Proposal: No (aligns with SQL standard)

4. **GPU acceleration:** Is DFA execution parallelizable?
   - Proposal: Yes for independent partitions (Phase 5)

5. **Integration with pg_ra_planner:** Should RPR rules be PostgreSQL extension?
   - Proposal: Core rules in RA, PG-specific in extension

---

## Next Steps

1. **Get feedback:** Share proposal with PostgreSQL hackers list
2. **Prototype:** Implement Phase 1 (Weeks 1-3) as proof of concept
3. **Benchmarks:** Create RPR benchmark suite (financial + fraud detection)
4. **Rules directory:** Create `rules/rpr/` with initial 15 rules
5. **Documentation:** Write RPR user guide with examples

**Estimated effort:** 20 weeks for full implementation
**Core functionality (Phases 1-4):** 12 weeks
**PostgreSQL integration ready:** Week 12


# Rule: Volcano Iterator Model - Iterator Fusion

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-iterator-fusion.rra`

## Metadata

- **ID:** `volcano-iterator-fusion`
- **Version:** 1.0.0
- **Databases:** postgresql, mysql, oracle, mssql, duckdb
- **Tags:** execution, iterator, volcano, fusion, optimization, compilation
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe, Thomas Neumann


# Volcano Iterator Model - Iterator Fusion

## Description

Iterator fusion merges adjacent pipelined operators into a single
combined operator, eliminating intermediate virtual function calls
and reducing per-tuple overhead. Instead of each operator independently
calling `next()` on its child, fused operators combine the logic of
multiple operators into a single tight loop.

**When to apply:** Fusion is beneficial when a chain of pipelined
operators (filter, project, etc.) creates excessive per-tuple call
overhead. The overhead is most visible on fast in-memory scans where
CPU cost dominates I/O.

**Why it works:** In a standard Volcano pipeline of depth D, each
tuple traverses D virtual function calls. Fusion collapses these into
a single function body, enabling the compiler to:
- Inline predicate evaluation
- Keep tuple fields in CPU registers
- Eliminate function call / return overhead (~5 ns each)
- Enable branch prediction and speculative execution

**Fusion strategies:**
- **Scan-Filter fusion**: Embed predicate into scan loop
- **Filter-Project fusion**: Evaluate predicate, then project in one pass
- **Scan-Filter-Project fusion**: Common three-way fusion
- **Full pipeline fusion**: Collapse entire pipeline segment between
  materializers into a single loop (HyPer-style compilation)

## Relational Algebra

```
Fusion rewrites:

// Before fusion: three separate iterators
Project(cols,
  Filter(pred,
    Scan(table)))

// Each next() call chain:
//   project.next() → filter.next() → scan.next()
//   3 virtual calls per tuple
//   filter may loop multiple times per output tuple

// After fusion: single combined iterator
ScanFilterProject(table, pred, cols)

// Single next() call:
//   loop { tuple = read(); if pred(tuple) { return project(tuple) } }
//   1 virtual call per output tuple
//   predicate and projection inlined

Fusible operator chains:
  Scan → Filter           → ScanFilter
  Filter → Project        → FilterProject
  Scan → Filter → Project → ScanFilterProject
  Filter → Filter         → Filter(p1 AND p2)
  Project → Project       → Project(compose(cols1, cols2))
  Limit → Filter          → FilterWithLimit
  Scan → Limit            → ScanWithLimit

Non-fusible boundaries:
  Sort (must materialize)
  HashAgg (must materialize)
  HashJoin build (must materialize)
  Exchange (thread boundary)
```

## Implementation

```rust
/// Iterator fusion: combines adjacent pipelined operators
/// into single operators to reduce per-tuple overhead.

/// Rewrite rule: fuse scan + filter into a single operator.
pub fn fuse_scan_filter(plan: &RelExpr) -> Option<RelExpr> {
    match plan {
        RelExpr::Filter {
            input,
            predicate,
        } => match input.as_ref() {
            RelExpr::Scan {
                table,
                filter: None,
            } => Some(RelExpr::Scan {
                table: table.clone(),
                filter: Some(predicate.clone()),
            }),
            RelExpr::Scan {
                table,
                filter: Some(existing),
            } => Some(RelExpr::Scan {
                table: table.clone(),
                filter: Some(Expr::And(
                    Box::new(existing.clone()),
                    Box::new(predicate.clone()),
                )),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// Rewrite rule: fuse adjacent filters into a single filter
/// with a conjunctive predicate.
pub fn fuse_filters(plan: &RelExpr) -> Option<RelExpr> {
    match plan {
        RelExpr::Filter {
            input,
            predicate: outer_pred,
        } => match input.as_ref() {
            RelExpr::Filter {
                input: inner_input,
                predicate: inner_pred,
            } => Some(RelExpr::Filter {
                input: inner_input.clone(),
                predicate: Expr::And(
                    Box::new(inner_pred.clone()),
                    Box::new(outer_pred.clone()),
                ),
            }),
            _ => None,
        },
        _ => None,
    }
}

/// Rewrite rule: fuse adjacent projections by composing
/// column lists.
pub fn fuse_projections(plan: &RelExpr) -> Option<RelExpr> {
    match plan {
        RelExpr::Project {
            input,
            columns: outer_cols,
        } => match input.as_ref() {
            RelExpr::Project {
                input: inner_input,
                columns: inner_cols,
            } => {
                // Outer project selects from inner project's output.
                // Compose: resolve outer column references through
                // inner column mappings.
                let composed =
                    compose_projections(outer_cols, inner_cols);
                Some(RelExpr::Project {
                    input: inner_input.clone(),
                    columns: composed,
                })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Fused scan-filter-project iterator.
/// Eliminates two levels of virtual dispatch.
pub struct ScanFilterProjectIterator {
    table: String,
    cursor: Option<TableCursor>,
    predicate: Expr,
    output_columns: Vec<ColumnRef>,
}

impl VolcanoIterator for ScanFilterProjectIterator {
    fn open(&mut self) -> Result<()> {
        self.cursor =
            Some(TableCursor::open(&self.table)?);
        Ok(())
    }

    fn next_tuple(&mut self) -> Result<Option<Tuple>> {
        let cursor =
            self.cursor.as_mut().expect("not opened");

        // Single tight loop: scan + filter + project
        loop {
            if !cursor.valid() {
                return Ok(None);
            }

            let tuple = cursor.current()?;
            cursor.advance()?;

            // Inline predicate evaluation
            if !eval_predicate(&self.predicate, &tuple)? {
                continue;
            }

            // Inline projection
            let projected =
                project_tuple(&tuple, &self.output_columns);
            return Ok(Some(projected));
        }
    }

    fn close(&mut self) -> Result<()> {
        if let Some(cursor) = self.cursor.take() {
            cursor.close()?;
        }
        Ok(())
    }

    fn schema(&self) -> &Schema {
        &self.output_schema
    }

    fn estimated_cardinality(&self) -> f64 {
        self.est_rows
    }
}

/// Apply all fusion rules to a plan tree (bottom-up).
pub fn apply_fusion(plan: &RelExpr) -> RelExpr {
    // First, recursively fuse children
    let plan = plan.map_children(|child| apply_fusion(child));

    // Try each fusion rule in priority order
    if let Some(fused) = fuse_scan_filter(&plan) {
        return fused;
    }
    if let Some(fused) = fuse_filters(&plan) {
        return fused;
    }
    if let Some(fused) = fuse_projections(&plan) {
        return fused;
    }

    plan
}

/// Estimate the speedup from fusion.
pub fn estimate_fusion_benefit(
    fused_depth: usize,
    original_depth: usize,
    tuples: f64,
) -> f64 {
    let call_overhead_ns = 8.0; // ~8ns per virtual call
    let original_cost =
        tuples * (original_depth as f64) * call_overhead_ns;
    let fused_cost =
        tuples * (fused_depth as f64) * call_overhead_ns;
    original_cost / fused_cost.max(1.0)
}
```

## Preconditions

- Adjacent operators are both fully pipelined (no materializer between)
- Predicate composition is semantically valid (AND conjunction)
- Column references in outer project resolve through inner project
- No side effects in operators being fused

## Cost Model

**Per-tuple savings from fusion:**
- Each eliminated operator level saves ~5-10 ns per tuple
- Scan-Filter fusion: saves 1 virtual call per tuple examined
- Scan-Filter-Project: saves 2 virtual calls per output tuple
- Full pipeline (depth D → 1): saves (D-1) calls per tuple

**Example: 10M row scan, depth-4 pipeline:**
- Unfused: 10M x 4 calls x 8ns = 320 ms overhead
- Fused to depth 1: 10M x 1 call x 8ns = 80 ms overhead
- Savings: 240 ms (75% reduction in protocol overhead)

**Additional benefits beyond call elimination:**
- Register allocation: tuple fields stay in registers
- Branch prediction: single loop body, predictable branches
- Instruction cache: smaller code footprint in hot loop
- Compiler optimization: inlining enables constant folding,
  dead code elimination across operator boundaries

**When fusion matters most:**
- In-memory databases (no I/O to mask CPU overhead)
- High selectivity filters (many tuples examined per output)
- Narrow tuples (per-tuple overhead dominates processing)
- Deep operator trees (many levels to collapse)

**When fusion matters least:**
- I/O-bound queries (disk latency >> call overhead)
- Pipeline breakers dominate (sort, hash build)
- Wide tuples with complex expressions per tuple

## Test Cases

```sql
-- Test 1: Scan-Filter fusion
SELECT * FROM orders WHERE status = 'shipped';
-- Before: Scan.next() → Filter.next(), 2 calls per tuple
-- After: ScanFilter.next(), 1 call per tuple
-- Speedup: ~2x on protocol overhead

-- Test 2: Scan-Filter-Project fusion
SELECT order_id, total FROM orders WHERE total > 100;
-- Before: Scan → Filter → Project, 3 calls per output tuple
-- After: ScanFilterProject, 1 call per output tuple
-- Verify: same results, fewer function calls

-- Test 3: Filter-Filter fusion (predicate conjunction)
SELECT * FROM users
WHERE age > 18
  AND region = 'US'
  AND active = true;
-- Parser may produce nested filters:
--   Filter(active, Filter(region, Filter(age, Scan)))
-- After fusion: Filter(age AND region AND active, Scan)
-- Then: ScanFilter(age AND region AND active)

-- Test 4: Fusion stops at materialization boundary
SELECT * FROM orders
WHERE status = 'shipped'
ORDER BY created_at;
-- Scan-Filter fuses, but Sort is a boundary
-- Pipeline 1: ScanFilter (fused) → Sort input
-- Pipeline 2: Sort output → Result
-- Verify: fusion applied within pipeline, not across Sort

-- Test 5: No fusion across join
SELECT o.*, c.name
FROM orders o JOIN customers c ON o.cust_id = c.id
WHERE o.total > 100;
-- Filter on orders fuses with scan: ScanFilter(orders)
-- Join remains separate operator
-- Project may fuse with join probe side

-- Negative test: fusion must not change semantics
SELECT DISTINCT name FROM users WHERE active = true;
-- Filter fuses with Scan
-- Distinct is a materializer, cannot fuse with filter
-- Verify: distinct still sees all qualifying rows
```

## References

1. **Neumann, Thomas**. "Efficiently Compiling Efficient Query Plans
   for Modern Hardware." PVLDB 4(9), 2011.
   - Full pipeline compilation (ultimate fusion)
   - Measures 10x speedup over Volcano on TPC-H

2. **Klonatos, Yannis et al**. "Building Efficient Query Engines in
   a High-Level Language." PVLDB 7(10), 2014.
   - LegoBase: staged compilation for operator fusion
   - Demonstrates fusion benefits in managed languages

3. **Shaikhha, Amir et al**. "How to Architect a Query Compiler,
   Revisited." SIGMOD 2018.
   - Systematic fusion via loop fusion transformations
   - Formal framework for correctness of fusion

4. **PostgreSQL Source**: `src/backend/optimizer/plan/createplan.c`
   - `create_scan_plan()` embeds filter into scan node
   - Practical scan-filter fusion

5. **Graefe, Goetz**. "Volcano: An Extensible and Parallel Query
   Evaluation System." IEEE TKDE 6(1), 1994.
   - Original iterator model that fusion optimizes

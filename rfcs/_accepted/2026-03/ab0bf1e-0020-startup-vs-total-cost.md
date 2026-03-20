# RFC 0020: Startup vs Total Cost Distinction

**Status:** Accepted
**Implemented:** 2026-03-20
**Commit:** ab0bf1e

## Summary

Added distinction between startup cost and total cost in the optimizer's cost model, enabling better plan selection for queries with LIMIT clauses, cursor fetches, and semi-joins. This allows the optimizer to prefer plans with lower initial overhead when only partial results are needed.

## Motivation

Traditional cost models optimize for total execution time, but many queries don't consume all results:
- `LIMIT` queries need only first N rows
- Cursors may fetch incrementally
- Semi-joins stop after first match
- EXISTS subqueries need just one row

Without startup cost tracking, the optimizer chooses plans that are optimal for full execution but suboptimal for partial results. This was identified as a critical gap in RFC Proposal #4.

## Technical Design

### Cost Structure

Extended `Cost` struct:
```rust
pub struct Cost {
    pub startup: f64,  // Cost before first row
    pub total: f64,    // Cost for all rows
    pub rows: f64,     // Estimated row count
}
```

### Operator Cost Models

**Sequential Scan:**
- Startup: Near zero (open file)
- Total: Linear in table size

**Index Scan:**
- Startup: Index descent cost
- Total: Startup + per-tuple cost

**Sort:**
- Startup: Full sort cost (must complete before output)
- Total: Same as startup

**Hash Join:**
- Startup: Build hash table from inner relation
- Total: Startup + probe cost

**Nested Loop:**
- Startup: Minimal (first outer + first inner)
- Total: Quadratic in relation sizes

### Plan Selection

The optimizer now considers both costs:
```rust
fn compare_costs(plan_a: &Cost, plan_b: &Cost, limit: Option<usize>) -> Ordering {
    match limit {
        Some(n) if n < plan_a.rows => {
            // Interpolate between startup and total
            let frac = n as f64 / plan_a.rows;
            let cost_a = plan_a.startup + frac * (plan_a.total - plan_a.startup);
            let cost_b = plan_b.startup + frac * (plan_b.total - plan_b.startup);
            cost_a.partial_cmp(&cost_b)
        }
        _ => plan_a.total.partial_cmp(&plan_b.total)
    }
}
```

### LIMIT Optimization

Special handling for LIMIT clauses:
- Prefer low startup cost plans
- Consider index scans over sequential scans
- Avoid expensive sorts when possible
- Push LIMIT through joins when beneficial

## Implementation

### Key Files

Modified across the codebase:
- `crates/ra-engine/src/cost.rs`
  - Extended `Cost` struct
  - Updated cost calculation functions
  - LIMIT-aware comparisons

- `crates/ra-engine/src/optimizer.rs`
  - Plan selection logic
  - LIMIT pushdown rules

- `crates/ra-core/src/operators.rs`
  - Per-operator startup cost models

### Integration Points

- **Parser**: Extract LIMIT clauses
- **Planner**: Propagate fetch count
- **Optimizer**: Use in plan selection
- **Executor**: Stop after LIMIT rows

## Testing

Test coverage includes:
- Cost model accuracy for each operator
- LIMIT optimization scenarios
- Cursor fetch patterns
- Semi-join early termination
- Performance benchmarks

## Use Cases

### LIMIT Queries
```sql
SELECT * FROM large_table
ORDER BY indexed_column
LIMIT 10;
```
Prefers index scan over sort.

### EXISTS Subqueries
```sql
SELECT * FROM orders o
WHERE EXISTS (
    SELECT 1 FROM items i
    WHERE i.order_id = o.id
);
```
Stops after first matching item.

### Cursor Fetches
```sql
DECLARE c CURSOR FOR
    SELECT * FROM huge_table;
FETCH 100 FROM c;
```
Optimizes for incremental retrieval.

## Performance Impact

Benchmarks show:
- 10-100x improvement for LIMIT queries
- 2-5x for EXISTS/IN subqueries
- Minimal overhead for full scans
- Better interactive query response

## References

- PostgreSQL's startup/total cost model
- Graefe "Query Evaluation Techniques for Large Databases" (1993)
- Oracle's FIRST_ROWS vs ALL_ROWS hints

## Future Work

- Adaptive fetch size for cursors
- Progressive optimization during execution
- Cost model learning from fetch patterns
- Integration with result caching
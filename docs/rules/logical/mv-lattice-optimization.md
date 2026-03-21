# Rule: Materialized View Lattice Optimization

**Category:** logical/view-rewriting
**File:** `rules/logical/view-rewriting/mv-lattice-optimization.rra`

## Metadata

- **ID:** `mv-lattice-optimization`
- **Version:** "1.0.0"
- **Databases:** oracle, snowflake, clickhouse, apache-kylin
- **Tags:** logical, materialized-view, lattice, olap, cube
- **Authors:** "Harinarayan, Rajaraman & Ullman"


# Materialized View Lattice Optimization

## Description

Given a lattice of possible aggregate views (data cube), selects which
views to materialize to minimize total query cost under a space budget.
The lattice captures the rollup relationships between GROUP BY sets:
view V1 can answer query Q if Q GROUP BY is a subset of V1 GROUP BY.

**When to apply**: OLAP workloads with many aggregate queries at
different granularities over the same fact table.

## Relational Algebra

```algebra
-- Lattice for dimensions {A, B, C}:
-- Level 3: {A,B,C} (base)
-- Level 2: {A,B}, {A,C}, {B,C}
-- Level 1: {A}, {B}, {C}
-- Level 0: {} (grand total)

-- Greedy: materialize views that provide maximum benefit per unit space
```

## Implementation

```rust
fn greedy_lattice_selection(
    lattice: &ViewLattice,
    space_budget: usize,
) -> Vec<ViewId> {
    let mut materialized = vec\![lattice.base_view()];
    let mut remaining_budget = space_budget;

    loop {
        let best = lattice.views()
            .filter(|v| \!materialized.contains(v))
            .filter(|v| v.estimated_size() <= remaining_budget)
            .max_by_key(|v| {
                let benefit = lattice.benefit_of_materializing(v, &materialized);
                benefit / v.estimated_size()
            });

        match best {
            Some(v) if lattice.benefit_of_materializing(&v, &materialized) > 0 => {
                remaining_budget -= v.estimated_size();
                materialized.push(v);
            }
            _ => break,
        }
    }
    materialized
}
```

## Preconditions

```rust
fn applicable(workload: &Workload) -> bool {
    workload.has_aggregate_queries()
        && workload.dimension_count() <= 20
        && workload.has_fact_table()
}
```

## Cost Model

```rust
fn benefit(
    view: &View,
    queries: &[Query],
    existing_mvs: &[View],
) -> f64 {
    queries.iter().map(|q| {
        let current_cost = q.best_cost_using(existing_mvs);
        let new_cost = q.best_cost_using_also(view, existing_mvs);
        (current_cost - new_cost).max(0.0) * q.frequency()
    }).sum()
}
```

## Test Cases

```sql
-- Lattice for sales cube
-- Dimensions: product, region, time_period
-- Queries at various granularities:

-- Q1: SELECT product, SUM(amount) GROUP BY product
-- Q2: SELECT region, time_period, SUM(amount) GROUP BY region, time_period
-- Q3: SELECT SUM(amount) -- grand total

-- Algorithm selects views to materialize based on query frequencies
-- and space budget
```

## References

- Harinarayan, V., Rajaraman, A. & Ullman, J.D., "Implementing Data Cubes Efficiently", ACM SIGMOD 1996, DOI: 10.1145/233269.233333
- THE seminal paper on view lattice optimization for OLAP

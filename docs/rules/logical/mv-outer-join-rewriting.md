# Rule: Materialized View Outer Join Rewriting

**Category:** logical/view-rewriting
**File:** `rules/logical/view-rewriting/mv-outer-join-rewriting.rra`

## Metadata

- **ID:** `mv-outer-join-rewriting`
- **Version:** "1.0.0"
- **Databases:** oracle, mssql, snowflake
- **Tags:** logical, materialized-view, outer-join, rewriting
- **Authors:** "Larson & Zhou, Microsoft Research"


# Materialized View Outer Join Rewriting

## Description

Extends MV rewriting to handle queries with outer joins. A view with
inner joins can answer outer join queries if the outer join can be
shown to produce no additional nulls (due to NOT NULL constraints or
WHERE clause filtering). Conversely, a view with outer joins can
answer inner join queries by filtering out null-extended rows.

**When to apply**: Query uses outer joins and an MV with related
joins (inner or outer) exists.

## Relational Algebra

```algebra
-- MV: R LEFT JOIN S ON R.id = S.rid (contains null-extended rows)
-- Query: R INNER JOIN S ON R.id = S.rid
-- Rewrite: sigma[S.rid IS NOT NULL](MV)
```

## Implementation

```rust
fn try_outer_join_rewrite(query: &Join, mv: &MaterializedView) -> Option<Plan> {
    match (query.join_type(), mv.join_type()) {
        (Inner, LeftOuter) => {
            Some(Plan::filter(
                Predicate::is_not_null(mv.nullable_side_key()),
                Plan::scan_mv(mv),
            ))
        }
        (LeftOuter, Inner) if query.has_not_null_constraint() => {
            Some(Plan::scan_mv(mv))
        }
        _ => None,
    }
}
```

## Preconditions

```rust
fn applicable(query: &Join, mv: &MaterializedView) -> bool {
    mv.tables_match(&query.tables())
        && mv.join_conditions_subsume(&query.conditions())
}
```

## Cost Model

```rust
fn estimated_benefit(mv_rows: f64, join_cost: f64) -> f64 {
    (join_cost - mv_rows * 0.001) / join_cost
}
```

## Test Cases

```sql
-- MV with LEFT JOIN
CREATE MATERIALIZED VIEW emp_dept AS
SELECT e.*, d.name AS dept_name
FROM employees e LEFT JOIN departments d ON e.dept_id = d.id;

-- Positive: inner join query answered by filtering MV
SELECT e.name, d.name FROM employees e
JOIN departments d ON e.dept_id = d.id;
-- Rewrite: SELECT name, dept_name FROM emp_dept WHERE dept_name IS NOT NULL;

-- Negative: different join condition
SELECT e.name, d.name FROM employees e
JOIN departments d ON e.manager_dept_id = d.id;
```

## References

- Larson, P. & Zhou, J., "View Matching for Outer-Join Views", VLDB 2007, DOI: 10.14778/1325851.1325896

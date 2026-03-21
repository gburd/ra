# Rule: Functional Dependency-Based Rewriting

**Category:** experimental/semantic
**File:** `rules/experimental/semantic/functional-dependency-rewrite.rra`

## Metadata

- **ID:** `functional-dependency-rewrite`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, cockroachdb
- **Tags:** semantic, functional-dependency, group-by, simplification
- **Authors:** "Simmen et al. 1996", "RA Contributors"


# Functional Dependency-Based Rewriting

## Description

Uses functional dependencies (FDs) derived from primary keys, unique
constraints, and join conditions to simplify queries. FDs enable GROUP BY
reduction (remove functionally determined columns), ORDER BY simplification,
DISTINCT elimination, and predicate implication.

**When to apply**: Queries with GROUP BY, ORDER BY, or DISTINCT clauses
where functional dependencies from the schema can reduce the number of
columns that need to be grouped, sorted, or deduplicated.

**Why it works**: If A -> B (A functionally determines B), then grouping
by (A, B) is equivalent to grouping by (A) alone. Similarly, sorting by
(A, B) when A -> B can be simplified to sorting by A. These simplifications
reduce hash table sizes, sort key widths, and comparison costs.

## Relational Algebra

```algebra
-- GROUP BY reduction via FD
aggregate[group_by: {A, B}, SUM(C)](R)
  -> aggregate[group_by: {A}, SUM(C)](R)
  where FD: A -> B in R

-- ORDER BY simplification
sort[A, B](R)
  -> sort[A](R)
  where FD: A -> B in R

-- Join-derived FD propagation
R join[R.id = S.rid] S
  -- Derives FD: S.rid -> R.* (from R.id being PK)
  -- Enables GROUP BY {S.rid, R.name} -> GROUP BY {S.rid}
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("fd-group-by-reduction";
    "(aggregate (group_by ?cols) ?aggs ?input)" =>
    "(aggregate (group_by (fd_reduce ?cols ?input)) ?aggs ?input)"
    if has_functional_dependencies("?input")
    if group_by_reducible("?cols", "?input")
),

rw!("fd-order-by-simplification";
    "(sort ?sort_keys ?input)" =>
    "(sort (fd_reduce_sort ?sort_keys ?input) ?input)"
    if sort_keys_reducible("?sort_keys", "?input")
),

rw!("fd-distinct-to-group-by";
    "(distinct (project ?cols ?input))" =>
    "(project ?cols ?input)"
    if projected_cols_form_key("?cols", "?input")
),
```

## Preconditions

```rust
fn applicable(
    query: &RelExpr,
    schema: &SchemaInfo,
) -> bool {
    // Compute functional dependencies from schema + query
    let fds = compute_functional_dependencies(query, schema);

    if fds.is_empty() {
        return false;
    }

    // Check if FDs can simplify any operator
    match query {
        RelExpr::Aggregate { group_by, .. } => {
            let minimal = fd_closure_minimal(group_by, &fds);
            minimal.len() < group_by.len()
        }
        RelExpr::Sort { keys, .. } => {
            let minimal = fd_closure_minimal(keys, &fds);
            minimal.len() < keys.len()
        }
        RelExpr::Distinct { input, .. } => {
            has_key_in_output(input, &fds)
        }
        _ => false,
    }
}

fn compute_functional_dependencies(
    query: &RelExpr,
    schema: &SchemaInfo,
) -> Vec<FunctionalDependency> {
    let mut fds = Vec::new();

    // From primary keys: PK -> all columns
    for table in query.referenced_tables() {
        if let Some(pk) = schema.primary_key(table) {
            for col in schema.columns(table) {
                fds.push(FunctionalDependency {
                    determinant: pk.clone(),
                    dependent: col,
                });
            }
        }
    }

    // From unique constraints
    for table in query.referenced_tables() {
        for unique in schema.unique_constraints(table) {
            for col in schema.columns(table) {
                fds.push(FunctionalDependency {
                    determinant: unique.clone(),
                    dependent: col,
                });
            }
        }
    }

    // From equi-join conditions: join key equivalence
    for join in query.join_conditions() {
        if join.is_equijoin() {
            // If R.id = S.rid, and R.id is PK, then S.rid -> R.*
            fds.extend(derive_join_fds(join, schema));
        }
    }

    fds
}
```

**Restrictions:**
- FDs must come from enforced constraints (not statistical correlations)
- Join-derived FDs require equi-join conditions
- Outer join nulls can invalidate FDs (left join does not preserve right FDs)
- Computed columns may introduce new FDs not in the schema

## Cost Model

```rust
fn estimated_benefit(
    original_group_by_cols: usize,
    reduced_group_by_cols: usize,
    stats: &Statistics,
) -> f64 {
    // Fewer group-by columns = smaller hash keys, fewer comparisons
    let key_width_ratio = reduced_group_by_cols as f64
        / original_group_by_cols as f64;

    // Hash table: fewer columns = less memory, faster hashing
    let hash_improvement = 1.0 - key_width_ratio;

    // Comparison: fewer columns = fewer comparisons per tuple
    let comparison_improvement = 1.0 - key_width_ratio;

    (hash_improvement + comparison_improvement) / 2.0
}
```

**Typical benefit**: 10-30% for GROUP BY reduction on wide tables.
DISTINCT elimination saves 50-90% when output already has a key.

## Test Cases

### Positive: GROUP BY reduction via PK

```sql
-- users.id is PRIMARY KEY, so id -> name, email
SELECT u.id, u.name, u.email, COUNT(o.id) AS order_count
FROM users u
JOIN orders o ON u.id = o.user_id
GROUP BY u.id, u.name, u.email;

-- Rewritten: GROUP BY u.id only (name, email functionally determined)
SELECT u.id, u.name, u.email, COUNT(o.id) AS order_count
FROM users u JOIN orders o ON u.id = o.user_id
GROUP BY u.id;
```

### Positive: ORDER BY simplification

```sql
-- departments.id -> departments.name
SELECT d.id, d.name, COUNT(*) AS emp_count
FROM departments d JOIN employees e ON d.id = e.dept_id
GROUP BY d.id, d.name
ORDER BY d.id, d.name;

-- Rewritten: ORDER BY d.id (name is determined by id)
```

### Negative: No FDs applicable

```sql
SELECT city, state, COUNT(*)
FROM addresses
GROUP BY city, state;

-- city does not determine state (multiple states have same city names)
-- No simplification possible
```

## References

**Academic papers:**
- Simmen et al., "Fundamental Techniques for Order Optimization", SIGMOD 1996
- Paulley, "Exploiting Functional Dependence in Query Optimization", PhD Thesis 2001
- Neumann, Moerkotte, "A Combined Framework for Grouping and Order Optimization", VLDB 2004

**Implementation:**
- MySQL: functional dependency checking for GROUP BY (SQL mode ONLY_FULL_GROUP_BY)
- PostgreSQL: primary key based GROUP BY reduction (since v9.1)
- CockroachDB: FD-based simplification in optimizer

**Key insights:**
- FD propagation through joins enables cross-table simplification
- Armstrong's axioms (reflexivity, augmentation, transitivity) compute FD closure
- Minimal cover computation removes redundant FDs
- SQL standard requires GROUP BY columns to be "functionally dependent" on key

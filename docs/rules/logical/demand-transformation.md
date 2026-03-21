# Rule: Demand Transformation (Generalized Magic Sets)

**Category:** logical/sideways-information-passing
**File:** `rules/logical/sideways-information-passing/demand-transformation.rra`

## Metadata

- **ID:** `demand-transformation`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle
- **Tags:** logical, demand, magic-sets, generalized, tabling
- **Authors:** "Tekle & Liu"


# Demand Transformation

## Description

Generalizes magic sets to handle non-linear recursion and stratified
negation. The demand transformation propagates queries through rules
to generate "demand" facts that restrict computation to only the
tuples relevant to answering the original query.

**When to apply**: Recursive queries with non-linear recursion or
stratified negation that cannot use standard magic sets.

## Relational Algebra

```algebra
-- Non-linear recursion: same_generation(X, Y)
-- sg(X, Y) :- flat(X, Y).
-- sg(X, Y) :- up(X, X1), sg(X1, Y1), down(Y1, Y).
-- Query: ?- sg("Alice", Y).
-- Demand transformation handles the double recursion variable
```

## Implementation

```rust
fn demand_transform(program: &DatalogProgram, query: &Query) -> DatalogProgram {
    let demand_rules = program.rules().iter()
        .flat_map(|rule| generate_demand_rules(rule, query))
        .collect();
    let guarded_rules = program.rules().iter()
        .map(|rule| add_demand_guard(rule))
        .collect();
    DatalogProgram::new(demand_seed(query), demand_rules, guarded_rules)
}
```

## Preconditions

```rust
fn applicable(query: &RecursiveQuery) -> bool {
    query.has_bound_arguments()
}
```

## Cost Model

```rust
fn estimated_benefit(full_eval: f64, demand_restricted: f64) -> f64 {
    (full_eval - demand_restricted) / full_eval
}
```

## Test Cases

```sql
-- Positive: same-generation query (non-linear)
-- Find all people in the same generation as Alice in an org hierarchy
WITH RECURSIVE sg AS (
    SELECT e.id FROM employees e WHERE e.name = 'Alice'
    UNION ALL
    SELECT peer.id FROM employees peer
    JOIN employees mgr ON peer.manager_id = mgr.id
    JOIN sg ON sg.id = mgr.id
)
SELECT * FROM sg;

-- Negative: no binding information
SELECT * FROM transitive_closure;
```

## References

- Tekle, K.T. & Liu, Y.A., "More Efficient Datalog Queries: Subsumptive Tabling Beats Magic Sets", ACM SIGMOD 2011, DOI: 10.1145/1989323.1989393

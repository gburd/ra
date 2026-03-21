# Rule: Constraint-Based Semantic Rewriting

**Category:** experimental/semantic
**File:** `rules/experimental/semantic/constraint-based-rewriting.rra`

## Metadata

- **ID:** `constraint-based-rewriting`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, cockroachdb, oracle
- **Tags:** semantic, constraints, integrity-constraints, chase
- **Authors:** "Chandra & Merlin 1977", "Siegel et al. 2002", "RA Contributors"


# Constraint-Based Semantic Rewriting

## Description

Exploits database integrity constraints (foreign keys, unique constraints,
NOT NULL, CHECK constraints) to simplify or eliminate query operations.
The Chase algorithm propagates constraints through the query to derive
implied predicates, remove redundant joins, and simplify expressions.

**When to apply**: Queries over schemas with rich integrity constraints
where the optimizer can prove that certain operations are redundant.
Common in ORMs that generate defensive queries (unnecessary outer joins,
redundant null checks).

**Why it works**: Integrity constraints are guaranteed by the database.
A foreign key R.a -> S.id means every R.a value exists in S.id.
This lets us eliminate the join R JOIN S ON R.a = S.id when only R
columns are needed (the join cannot filter any R rows). Similarly,
NOT NULL constraints eliminate IS NULL checks.

## Relational Algebra

```algebra
-- FK join elimination (when only left columns needed)
project[R.*](R join[R.a = S.id] S)
  -> R
  where FK(R.a -> S.id) AND NOT NULL(R.a)

-- Implied predicate from FK
filter[R.a = 5](R join[R.a = S.id] S)
  -> filter[R.a = 5](R) join[R.a = S.id] filter[S.id = 5](S)
  -- FK guarantees S.id = 5 exists if R.a = 5 passes

-- NOT NULL simplification
filter[R.a IS NOT NULL](R)
  -> R
  where NOT NULL(R.a)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("fk-join-elimination";
    "(project ?cols (join (eq ?fk ?pk) ?left ?right))" =>
    "(project ?cols ?left)"
    if foreign_key_exists("?fk", "?pk")
    if not_null_constraint("?fk")
    if cols_only_from_left("?cols", "?left")
),

rw!("fk-implied-predicate";
    "(filter (eq ?fk ?val)
       (join (eq ?fk ?pk) ?left ?right))" =>
    "(join (eq ?fk ?pk)
       (filter (eq ?fk ?val) ?left)
       (filter (eq ?pk ?val) ?right))"
    if foreign_key_exists("?fk", "?pk")
),

rw!("not-null-elimination";
    "(filter (is_not_null ?col) ?input)" =>
    "?input"
    if has_not_null_constraint("?col", "?input")
),

rw!("check-constraint-simplification";
    "(filter ?pred ?input)" =>
    "?input"
    if check_constraint_implies("?input", "?pred")
),

rw!("unique-constraint-distinct-elim";
    "(distinct ?input)" =>
    "?input"
    if output_has_unique_key("?input")
),
```

## Preconditions

```rust
fn applicable(
    query: &RelExpr,
    schema: &SchemaInfo,
) -> bool {
    // Schema must have integrity constraints defined
    let constraints = schema.get_constraints();

    if constraints.is_empty() {
        return false;
    }

    // At least one constraint is applicable to this query
    constraints.iter().any(|c| is_relevant(c, query))
}
```

**Restrictions:**
- Requires accurate schema metadata (constraints must be enforced)
- Deferred constraints may invalidate rewrites within transactions
- CHECK constraints with complex expressions need expression analysis
- Not applicable to unvalidated constraints (NOVALIDATE in Oracle)

## Cost Model

```rust
fn estimated_benefit(
    original: &RelExpr,
    simplified: &RelExpr,
    stats: &Statistics,
) -> f64 {
    // Constraint-based rewrites remove operations entirely
    // Benefit is the cost of the eliminated operation
    let original_cost = estimate_cost(original, stats);
    let simplified_cost = estimate_cost(simplified, stats);

    if original_cost > simplified_cost {
        (original_cost - simplified_cost) / original_cost
    } else {
        0.0
    }
}
```

**Typical benefit**: 30-70% when ORM-generated queries include
redundant joins. DISTINCT elimination saves sort/hash cost.

## Test Cases

### Positive: FK join elimination

```sql
-- orders.customer_id references customers(id) NOT NULL
SELECT o.id, o.amount
FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- Rewritten: SELECT o.id, o.amount FROM orders o;
-- Join is redundant: FK guarantees customer exists, only o columns needed
```

### Positive: NOT NULL constraint simplification

```sql
-- Column defined as NOT NULL
SELECT * FROM users WHERE email IS NOT NULL;

-- Rewritten: SELECT * FROM users;
-- NOT NULL constraint guarantees no nulls
```

### Positive: Unique key DISTINCT elimination

```sql
SELECT DISTINCT u.id, u.name FROM users u;

-- Rewritten: SELECT u.id, u.name FROM users u;
-- u.id is PRIMARY KEY, so output is already unique
```

### Negative: No relevant constraints

```sql
SELECT * FROM log_entries WHERE message LIKE '%error%';
-- No constraints on log_entries relevant to this query
```

## References

**Academic papers:**
- Chandra, Merlin, "Optimal Implementation of Conjunctive Queries in Relational Data Bases", STOC 1977
- Siegel et al., "Using Constraints to Optimize Query Processing", TKDE 2002
- Deutsch et al., "The Chase Revisited", PODS 2008

**Implementation:**
- PostgreSQL: constraint-aware join elimination (since v12)
- Oracle: constraint-based query rewriting for materialized views
- mssql: foreign key trust for join elimination

**Key insights:**
- Chase algorithm systematically propagates constraints through queries
- Modern ORMs (Django, Rails, Hibernate) generate many eliminable joins
- Constraint-based optimization is compositional (each constraint independent)
- Requires schema introspection at optimization time

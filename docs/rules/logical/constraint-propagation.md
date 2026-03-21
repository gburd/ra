# Rule: Constraint Propagation for Query Simplification

**Category:** logical/sideways-information-passing
**File:** `rules/logical/sideways-information-passing/constraint-propagation.rra`

## Metadata

- **ID:** `constraint-propagation`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, mssql, db2
- **Tags:** logical, constraint, propagation, check, foreign-key, simplification
- **Authors:** "Pirahesh, Hasan", "Hellerstein, Joseph"


# Constraint Propagation for Query Simplification

## Description

Exploits database integrity constraints (CHECK, NOT NULL, FOREIGN KEY,
UNIQUE) to simplify or eliminate predicates and joins. A CHECK constraint
`age >= 0` makes the predicate `age >= 0` redundant. A FOREIGN KEY
constraint guarantees a join will not eliminate rows from the referencing
table, potentially allowing join elimination.

**When to apply**: Queries with predicates or joins that are implied by
declared integrity constraints.

## Relational Algebra

```algebra
-- Before: redundant predicate (CHECK age >= 0 exists)
sigma[age >= 0](employees)

-- After: predicate eliminated
employees

-- Before: FK-guaranteed join
pi[o.id, o.amount](orders o JOIN customers c ON o.cust_id = c.id)

-- After: join eliminated (FK guarantees match, only order cols used)
pi[id, amount](orders)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Remove predicates implied by CHECK constraints
rw!("constraint-implied-predicate";
    "(filter ?pred ?rel)" =>
    "?rel"
    if predicate_implied_by_check("?pred", "?rel")
),

// Eliminate FK-guaranteed inner join when only referencing cols used
rw!("fk-join-elimination";
    "(project ?cols (join ?key ?referencing ?referenced))" =>
    "(project ?cols ?referencing)"
    if fk_guarantees_match("?referencing", "?referenced", "?key")
    if cols_only_from("?cols", "?referencing")
),
```

## Preconditions

```rust
fn applicable(query: &Query, catalog: &Catalog) -> bool {
    let constraints = catalog.constraints_for(query.tables());
    // At least one constraint is exploitable
    constraints.iter().any(|c| match c {
        Constraint::Check(pred) =>
            query.has_implied_predicate(pred),
        Constraint::ForeignKey { from, to } =>
            query.has_eliminable_join(from, to),
        Constraint::NotNull(col) =>
            query.has_redundant_null_check(col),
        _ => false,
    })
}
```

**Restrictions:**
- Constraints must be validated (not `NOT VALID`)
- Deferred constraints cannot be relied upon mid-transaction
- Outer joins are not eliminable via FK alone

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    predicates_eliminated: usize,
    joins_eliminated: usize,
) -> f64 {
    let pred_savings = rows * predicates_eliminated as f64 * 0.01;
    let join_savings = rows * joins_eliminated as f64 * 0.5;
    pred_savings + join_savings
}
```

**Typical benefit**: 10-60%, especially for ORM-generated queries with
redundant joins.

## Test Cases

```sql
-- Positive: CHECK constraint eliminates predicate
-- Given: CHECK (status IN ('active', 'inactive'))
SELECT * FROM users WHERE status IN ('active', 'inactive');
-- Simplifies to: SELECT * FROM users

-- Positive: FK join elimination
-- Given: orders.cust_id REFERENCES customers(id)
SELECT o.id, o.amount FROM orders o
  JOIN customers c ON o.cust_id = c.id;
-- Simplifies to: SELECT id, amount FROM orders

-- Negative: outer join not eliminable
SELECT o.id, c.name FROM orders o
  LEFT JOIN customers c ON o.cust_id = c.id;
```

## References

- Pirahesh, H. et al. "Extensible/Rule Based Query Rewrite Optimization in Starburst" (SIGMOD 1992)
- Galindo-Legaria, C. "Algebraic Optimization of Outerjoin Queries" (PhD thesis, 1992)

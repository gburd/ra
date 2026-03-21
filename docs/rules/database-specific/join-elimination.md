# Rule: Oracle Join Elimination

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/join-elimination.rra`

## Metadata

- **ID:** `oracle-join-elimination`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, join, elimination, foreign-key, redundant
- **Authors:** "RA Contributors"


# Oracle Join Elimination

## Description

Eliminates redundant joins when the joined table does not contribute
any columns to the query result and a foreign key constraint guarantees
referential integrity.  Oracle's optimizer detects that the join is
only validating existence (which the FK already guarantees) and removes
it entirely.

**When to apply**: A table in a join is not referenced in the SELECT
list, and a foreign key from the referencing table to the referenced
table ensures all join matches exist.

**Why it works**: If table A has a NOT NULL foreign key referencing
table B's primary key, every row in A has exactly one match in B.
If the query only selects columns from A, the join with B is redundant
and can be eliminated, avoiding the join computation entirely.

**Database version**: Oracle 10gR2+

## Relational Algebra

```algebra
-- Before: redundant join (only A's columns used)
pi[A.cols](A join[A.fk = B.pk] B)

-- After: join eliminated
pi[A.cols](A)
  where FK(A.fk -> B.pk) is NOT NULL
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-join-elimination-fk";
    "(project ?cols (join inner (eq ?fk ?pk) ?referencing ?referenced))" =>
    "(project ?cols ?referencing)"
    if is_database("oracle")
    if cols_only_from("?cols", "?referencing")
    if has_fk_constraint("?referencing", "?fk", "?referenced", "?pk")
    if column_is_not_null("?referencing", "?fk")
),
```

## Preconditions

```rust
fn applicable(
    query_columns: &HashSet<Column>,
    referenced_table: &Table,
    fk: &ForeignKey,
) -> bool {
    query_columns.iter().all(|c| !c.belongs_to(referenced_table))
    && fk.is_validated()
    && fk.referencing_column().is_not_null()
}
```

**Restrictions:**
- Foreign key must be validated (RELY or VALIDATED state)
- FK column must be NOT NULL (nullable FK means some rows have no match)
- Outer joins may still be eliminable under different conditions
- OPTIMIZER_FEATURES_ENABLE must include join elimination

## Cost Model

```rust
fn estimated_benefit(
    eliminated_table_rows: f64,
    join_type_cost: f64,
) -> f64 {
    eliminated_table_rows * join_type_cost
}
```

**Typical benefit**: Eliminates an entire table scan and join
operation, proportional to the eliminated table's size.

## Test Cases

```sql
-- Positive: FK guarantees match, only orders columns used
SELECT o.id, o.amount FROM orders o
JOIN customers c ON o.customer_id = c.id;
-- c not in SELECT; FK customer_id -> c.id; join eliminated
```

```sql
-- Negative: need column from joined table
SELECT o.id, c.name FROM orders o
JOIN customers c ON o.customer_id = c.id;
-- c.name is needed; cannot eliminate join
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Join Elimination"
Oracle: ELIMINATE_JOIN / NO_ELIMINATE_JOIN hints

# Rule: "Split Joins with Disjunctive Predicates"

**Category:** logical/join-split
**File:** `rules/database-specific/cockroachdb/cockroachdb-split-disjunctive-joins.rra`

## Metadata

- **ID:** `cockroachdb-split-disjunctive-joins`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Split Joins with Disjunctive Predicates

## Description

Transforms joins with OR conditions into UNION ALL of multiple joins.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(inner-join t1 t2 (or (eq a b) (eq c d)))

-- After
(union-all (inner-join t1 t2 (eq a b)) (inner-join t1 t2 (eq c d)))
```

## Preconditions

- ON clause contains OR with disjoint column sets

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation

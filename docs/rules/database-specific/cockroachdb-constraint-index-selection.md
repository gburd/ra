# Rule: "Constrain Index Selection via Predicates"

**Category:** physical/index-selection
**File:** `rules/database-specific/cockroachdb/cockroachdb-constraint-index-selection.rra`

## Metadata

- **ID:** `cockroachdb-constraint-index-selection`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Constrain Index Selection via Predicates

## Description

Derives constraints from WHERE clause and applies to index selection.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(filter (scan orders) (and (eq region 'us-east') (gt total 100)))

-- After
(filter (index-scan orders idx_region_total) (and (eq region 'us-east') (gt total 100)))
```

## Preconditions

- Index supports derived constraints from predicates

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation

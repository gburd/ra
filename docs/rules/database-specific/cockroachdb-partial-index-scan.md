# Rule: "Generate Partial Index Scans"

**Category:** physical/index-selection
**File:** `rules/database-specific/cockroachdb/cockroachdb-partial-index-scan.rra`

## Metadata

- **ID:** `cockroachdb-partial-index-scan`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Generate Partial Index Scans

## Description

Generates unconstrained index scans over partial indexes with predicates implied by filters.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(scan orders WHERE status \!= 'cancelled')

-- After
(index-scan orders_active_idx)
```

## Preconditions

- Partial index WHERE predicate is implied by query WHERE clause

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation

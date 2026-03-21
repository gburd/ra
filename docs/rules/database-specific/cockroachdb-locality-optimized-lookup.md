# Rule: "Locality Optimized Search for Lookup Joins"

**Category:** distributed/locality
**File:** `rules/database-specific/cockroachdb/cockroachdb-locality-optimized-lookup.rra`

## Metadata

- **ID:** `cockroachdb-locality-optimized-lookup`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Locality Optimized Search for Lookup Joins

## Description

Optimizes lookup joins for geo-distributed tables by preferring local replicas.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(lookup-join (scan orders) (index-scan customers))

-- After
(locality-optimized-search (lookup-join (scan orders) (index-scan customers)))
```

## Preconditions

- Input is REGIONAL BY ROW table

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation

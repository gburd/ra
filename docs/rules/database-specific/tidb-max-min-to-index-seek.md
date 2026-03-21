# Rule: "MAX/MIN to Index Seek Conversion"

**Category:** physical/aggregate-optimization
**File:** `rules/database-specific/tidb/tidb-max-min-to-index-seek.rra`

## Metadata

- **ID:** `tidb-max-min-to-index-seek`
- **Version:** "1.0.0"
- **Databases:** tidb
- **Tags:** database-mining, tidb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# MAX/MIN to Index Seek Conversion

## Description

Replaces MAX() and MIN() with single index seek instead of full scan.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(aggregate (max id) (scan orders))

-- After
(project (index-seek orders idx_id DESC LIMIT 1))
```

## Preconditions

- Ascending/descending index exists on aggregated column

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation

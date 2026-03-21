# Rule: "CockroachDB Inverted Join"

**Category:** physical/join-selection
**File:** `rules/database-specific/cockroachdb/cockroachdb-inverted-join.rra`

## Metadata

- **ID:** `cockroachdb-inverted-join`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** database-mining, cockroachdb, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# CockroachDB Inverted Join

## Description

Generates lookup joins using inverted indexes for geospatial (ST_DWithin) and JSON containment predicates.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(inner-join (scan docs) (scan regions) (st-dwithin location bounds))

-- After
(inverted-join (scan docs) (inverted-index-scan regions))
```

## Preconditions

- Inverted index exists on JSON/geospatial column

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation

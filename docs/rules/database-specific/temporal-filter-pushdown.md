# Rule: Materialize Temporal Filter Pushdown

**Category:** database-specific/materialize
**File:** `rules/database-specific/materialize/temporal-filter-pushdown.rra`

## Metadata

- **ID:** `materialize-temporal-filter-pushdown`
- **Version:** "1.0.0"
- **Databases:** materialize
- **Tags:** database-specific, materialize, temporal, filter, pushdown, streaming
- **Authors:** "RA Contributors"


# Materialize Temporal Filter Pushdown

## Description

Pushes temporal predicates (mz_now(), event_time comparisons) into
source reads to limit the time range of data materialized from
differential dataflow arrangements.  Materialize's optimizer
recognizes temporal filters and translates them into `since` and
`until` frontiers on the underlying arrangement.

**When to apply**: A query filters on a temporal column or uses
mz_now() to restrict to recent data.

**Why it works**: Materialize maintains versioned arrangements
(indexed differential dataflow collections).  Temporal filters
restrict which versions are materialized, reducing memory for
arrangements and avoiding processing of historical data that will
be immediately discarded.

**Database version**: Materialize 0.40+

## Relational Algebra

```algebra
-- Before: materialize all, then filter
sigma[event_time > mz_now() - INTERVAL '1 hour'](arrangement(S))

-- After: temporal frontier applied to arrangement
arrangement(S, since = mz_now() - INTERVAL '1 hour')
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("materialize-temporal-filter-pushdown";
    "(filter (gt ?time_col (subtract (mz-now) ?interval))
            (arrange ?source))" =>
    "(arrange-with-since ?source (subtract (mz-now) ?interval))"
    if is_database("materialize")
    if is_temporal_column("?time_col")
),
```

## Preconditions

```rust
fn applicable(pred: &Expr, source: &Source) -> bool {
    pred.references_mz_now()
    && source.supports_temporal_frontier()
}
```

**Restrictions:**
- Only works with mz_now() or explicit temporal columns
- Non-temporal predicates use standard filter pushdown
- Temporal filters must be monotonic (no lookback into compacted data)

## Cost Model

```rust
fn estimated_benefit(
    total_versions: f64,
    filtered_versions: f64,
    arrangement_size_bytes: f64,
) -> f64 {
    let fraction_eliminated =
        (total_versions - filtered_versions) / total_versions;
    fraction_eliminated * arrangement_size_bytes
}
```

**Typical benefit**: For a 30-day retention with a 1-hour filter,
eliminates ~99.9% of arrangement data.

## Test Cases

```sql
-- Positive: mz_now() temporal filter
CREATE MATERIALIZED VIEW recent_events AS
SELECT * FROM events WHERE event_time > mz_now() - INTERVAL '1 hour';
-- Arrangement only holds last hour of data
```

```sql
-- Negative: non-temporal filter
SELECT * FROM events WHERE status = 'active';
-- Standard filter, no temporal optimization
```

## References

Materialize: src/transform/src/predicate_pushdown.rs
Materialize: src/compute-types/src/plan/interpret/mod.rs

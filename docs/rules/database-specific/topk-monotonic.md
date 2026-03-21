# Rule: Materialize Monotonic TopK

**Category:** database-specific/materialize
**File:** `rules/database-specific/materialize/topk-monotonic.rra`

## Metadata

- **ID:** `materialize-topk-monotonic`
- **Version:** "1.0.0"
- **Databases:** materialize
- **Tags:** database-specific, materialize, topk, monotonic, streaming, limit
- **Authors:** "RA Contributors"


# Materialize Monotonic TopK

## Description

Optimizes TopK (ORDER BY + LIMIT) on monotonic (append-only) inputs
by using a specialized operator that avoids full retraction tracking.
For monotonic inputs, the TopK operator only needs to track the
current top-K rows and emit changes when new rows enter the window.

**When to apply**: A TopK (LIMIT with ORDER BY) operates on a
monotonic input such as a Kafka source or append-only table.

**Why it works**: Standard TopK in differential dataflow must handle
arbitrary retractions, maintaining full state.  Monotonic TopK knows
rows are never retracted, so it uses a simpler min-heap that only
grows and evicts, reducing memory and update cost.

**Database version**: Materialize 0.30+

## Relational Algebra

```algebra
-- Before: standard TopK
topk[ORDER BY ts DESC LIMIT k](source_monotonic)

-- After: monotonic TopK
monotonic-topk[ORDER BY ts DESC LIMIT k](source_monotonic)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("materialize-monotonic-topk";
    "(topk ?order ?limit ?input)" =>
    "(monotonic-topk ?order ?limit ?input)"
    if is_database("materialize")
    if is_monotonic("?input")
),
```

## Preconditions

```rust
fn applicable(
    input: &MirRelationExpr,
    limit: Option<usize>,
) -> bool {
    input.is_monotonic() && limit.is_some()
}
```

**Restrictions:**
- Input must be strictly monotonic (no updates or deletes)
- OFFSET is not supported with monotonic optimization
- Group-by TopK (DISTINCT ON) has separate handling

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    limit_k: usize,
    updates_per_sec: f64,
) -> f64 {
    // Standard: O(rows) state
    // Monotonic: O(k) state per group
    let memory_saved = (rows - limit_k as f64) * 32.0;
    let update_saved = updates_per_sec * 0.005;
    memory_saved + update_saved
}
```

**Typical benefit**: For LIMIT 10 on a 10M-row monotonic source,
reduces state from 10M rows to 10 rows per group.

## Test Cases

```sql
-- Positive: TopK on Kafka source
CREATE SOURCE events FROM KAFKA ...;
CREATE MATERIALIZED VIEW latest_events AS
SELECT * FROM events ORDER BY event_time DESC LIMIT 100;
-- Uses monotonic TopK: only tracks top 100
```

```sql
-- Negative: TopK on mutable table
CREATE MATERIALIZED VIEW top_users AS
SELECT * FROM users ORDER BY score DESC LIMIT 10;
-- users table has updates; must use standard TopK
```

## References

Materialize: src/compute/src/render/top_k.rs
Materialize: src/transform/src/monotonic.rs

# Rule: Materialize Watermark-Based Frontier Propagation

**Category:** database-specific/materialize
**File:** `rules/database-specific/materialize/watermark-propagation.rra`

## Metadata

- **ID:** `materialize-watermark-propagation`
- **Version:** "1.0.0"
- **Databases:** materialize
- **Tags:** watermark, frontier, compaction, timestamp, differential
- **Authors:** "Materialize Inc."


# Materialize Watermark-Based Frontier Propagation

## Description

Propagates time frontiers (watermarks) through the dataflow graph to enable
compaction of differential state. Each operator maintains a frontier indicating
the minimum timestamp at which future updates may arrive. When the frontier
advances past a timestamp, all state associated with that timestamp can be
compacted (merged), reducing memory usage and speeding up future lookups.

**When to apply**: All Materialize dataflows benefit from frontier propagation.
The optimizer maximizes frontier advancement by analyzing the dataflow topology
to determine the tightest possible frontier for each operator, enabling earliest
possible compaction.

**Why it works**: Differential dataflow maintains state indexed by timestamp.
Without compaction, state grows linearly with the number of distinct timestamps.
Frontier propagation tells each operator "no updates with timestamp < T will
ever arrive", allowing it to merge all state at timestamps < T into a single
consolidated entry. This keeps arrangement sizes proportional to the number of
distinct values, not the number of updates.

## Relational Algebra

```algebra
-- Differential state without compaction:
arrangement = {
  (key="alice", val=1, time=100, diff=+1),
  (key="alice", val=1, time=200, diff=-1),
  (key="alice", val=2, time=200, diff=+1),
  (key="alice", val=2, time=300, diff=-1),
  (key="alice", val=3, time=300, diff=+1)
}

-- After frontier advances to 300 and compaction:
arrangement = {
  (key="alice", val=3, time=300, diff=+1)
}
-- 5 entries compacted to 1
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("materialize-advance-frontier";
    "(operator ?op
       (frontier ?current)
       (inputs ?in1 ?in2))" =>
    "(operator ?op
       (frontier (min-frontier ?in1 ?in2))
       (inputs ?in1 ?in2))"
    if frontier_can_advance("?current", "?in1", "?in2")
),

rw!("materialize-compaction-trigger";
    "(arrangement ?coll ?key
       (frontier ?f)
       (state ?entries))" =>
    "(arrangement ?coll ?key
       (frontier ?f)
       (compact-at ?entries ?f))"
    if state_has_old_timestamps("?entries", "?f")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> bool {
    stats.has_differential_state
        && stats.frontier_lag > 0  // frontier behind source time
        && stats.state_timestamp_spread > 1
}
```

**Restrictions:**
- Frontier cannot advance past the minimum of all input frontiers
- Slow consumers hold back frontier advancement for upstream operators
- Compaction is an I/O-intensive operation (rewriting arrangement pages)
- Too-frequent compaction wastes CPU; too-rare wastes memory

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    let total_entries = stats.arrangement_entries as f64;
    let distinct_values = stats.distinct_keys as f64;
    let timestamp_spread = stats.distinct_timestamps as f64;

    // Without compaction: entries proportional to updates
    let uncompacted_size = total_entries;

    // With compaction: entries proportional to distinct values
    let compacted_size = distinct_values;

    if uncompacted_size > compacted_size {
        (uncompacted_size - compacted_size) / uncompacted_size
    } else {
        0.0
    }
}
```

**Typical benefit**: 20% to 5x memory reduction depending on update frequency.

## Test Cases

### Positive: High-frequency updates with compaction

```sql
-- Sensor readings: 1000 updates/sec per sensor, 100 sensors
CREATE SOURCE readings FROM KAFKA CONNECTION kafka_conn
  (TOPIC 'readings') FORMAT AVRO
  ENVELOPE UPSERT;

CREATE MATERIALIZED VIEW latest_readings AS
SELECT sensor_id, value
FROM readings;

-- Without compaction: 100K entries/sec accumulate
-- With frontier at current_time - 1s:
-- Only 100 entries (one per sensor) maintained after compaction
-- 1000x memory reduction
```

### Positive: Join frontier propagation

```sql
-- Two sources joined: frontier is min of both
CREATE MATERIALIZED VIEW enriched_events AS
SELECT e.*, u.name
FROM events e JOIN users u ON e.user_id = u.id;

-- events frontier: time 1000 (fast source)
-- users frontier: time 950 (slow source)
-- join frontier: min(1000, 950) = 950
-- Compaction at 950 for both arrangements
```

### Negative: Blocked frontier

```sql
-- Source with no progress: frontier stuck at 0
-- Prevents compaction of all downstream operators
-- All state accumulates indefinitely

-- Diagnosis: check frontier with
-- SELECT * FROM mz_internal.mz_frontiers;
```

## References

**Implementation:**
- Materialize source: `src/compute/src/compute_state.rs` (frontier tracking)
- Compaction: `src/storage/src/source/antichain.rs`
- Timely dataflow: `timely/src/progress/frontier.rs`

**Documentation:**
- Materialize docs: "Understanding Materialize Internals"
- Timely dataflow: "Progress Tracking"

**Papers:**
- Murray, D.G., et al., "Naiad: A Timely Dataflow System", SOSP 2013
  - DOI: 10.1145/2517349.2522738
- McSherry, F., et al., "differential-dataflow", CIDR 2013

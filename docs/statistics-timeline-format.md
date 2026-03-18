# Statistics Timeline Format

The statistics timeline format is a TOML-based specification for describing how database statistics evolve over time. It drives the `TimelinePlayer` engine for stepping through snapshots to demonstrate adaptive query optimization.

## Format Structure

A timeline file has four top-level sections:

```toml
[metadata]        # Required: name, description, database context
[[snapshots]]     # Required: ordered statistics snapshots
[[events]]        # Optional: data modification events
[[feedback]]      # Optional: execution feedback (estimated vs actual)
```

## Metadata

```toml
[metadata]
name = "tpch-q1-evolution"
description = "TPC-H Q1 lineitem statistics over batch inserts"
database = "postgresql"          # optional
schema = "TPC-H"                 # optional
scale_factor = 1.0               # optional
duration_seconds = 3600          # optional
```

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Unique identifier for the timeline |
| `description` | yes | Human-readable description |
| `database` | no | Target database system |
| `schema` | no | Schema or benchmark name |
| `scale_factor` | no | Benchmark scale factor |
| `duration_seconds` | no | Total simulated duration |

## Snapshots

Snapshots are ordered by `time_offset` (seconds from timeline start). Each snapshot contains per-table statistics.

```toml
[[snapshots]]
time_offset = 0
label = "initial load"

[[snapshots.tables]]
name = "lineitem"
row_count = 6001215
page_count = 80000              # optional, estimated if absent
avg_row_size = 127.0            # optional, defaults to 100.0
table_size_bytes = 762154305    # optional, estimated if absent

[[snapshots.tables.columns]]
name = "l_orderkey"
ndv = 1500000
null_fraction = 0.0             # defaults to 0.0
avg_width = 8.0                 # defaults to 8.0
correlation = 0.98              # optional
min_value = "1"                 # optional, for display
max_value = "1500000"           # optional, for display
```

### Table fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Table name |
| `row_count` | yes | Total row count |
| `page_count` | no | Disk pages (estimated from row_count if absent) |
| `avg_row_size` | no | Average row size in bytes |
| `table_size_bytes` | no | Total table size in bytes |

### Column fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Column name |
| `ndv` | yes | Number of distinct values |
| `null_fraction` | no | Fraction of NULLs (0.0 to 1.0) |
| `avg_width` | no | Average width in bytes (default 8.0) |
| `correlation` | no | Physical correlation (-1.0 to 1.0) |
| `min_value` | no | Min value (string, for documentation) |
| `max_value` | no | Max value (string, for documentation) |

### Constraints

- At least one snapshot is required
- `time_offset` values must be strictly ascending
- No duplicate `time_offset` values

## Events

Events mark data modifications and system actions between snapshots.

```toml
[[events]]
time_offset = 300
kind = "insert"
table = "lineitem"
row_count = 500000              # optional
description = "Batch load"      # optional
```

### Event kinds

| Kind | Description |
|------|-------------|
| `insert` | Bulk insert |
| `update` | Bulk update |
| `delete` | Bulk delete |
| `analyze` | Statistics refresh (ANALYZE) |
| `reoptimize` | Optimizer triggered replanning |
| `schema_change` | DDL change (add column, index) |
| `vacuum` | Dead tuple reclamation |

## Execution Feedback

Feedback entries compare optimizer estimates against actual execution results.

```toml
[[feedback]]
time_offset = 700
query = "SELECT ... FROM lineitem WHERE ..."
operator = "SeqScan on lineitem"    # optional
estimated_rows = 5916591.0
actual_rows = 6408591.0
estimated_cost = 1500000.0          # optional
actual_time_ms = 2350.0             # optional
```

The `q_error` metric is computed as `max(estimated/actual, actual/estimated)`, where 1.0 means a perfect estimate.

## TimelinePlayer API

```rust
use ra_stats::timeline::{Timeline, TimelinePlayer, PlaybackState};

// Parse from TOML string
let timeline = Timeline::from_toml(toml_str)?;

// Create player
let mut player = TimelinePlayer::new(timeline)?;

// Navigate
player.step_forward();          // BeforeStart -> AtSnapshot(0)
player.step_forward();          // AtSnapshot(0) -> AtSnapshot(1)
player.step_backward();         // AtSnapshot(1) -> AtSnapshot(0)
player.seek(2)?;                // Jump to index 2
player.seek_to_time(600);       // Jump to nearest snapshot at t=600
player.seek_start();            // First snapshot
player.seek_end();              // Last snapshot
player.reset();                 // Back to BeforeStart

// Query current state
if let Some(snap) = player.current_snapshot() {
    let stats = snap.to_managed_stats();
    // stats: HashMap<String, ManagedTableStats>
}

// Events and feedback
let events = player.events_until_next();
let feedback = player.feedback_at_current();

// Analytics
let delta = player.row_count_delta("lineitem", 0, 1);
let avg_q = player.average_q_error();
let max_q = player.max_q_error();
```

## Example Timelines

The `timelines/` directory contains example files:

### Basic scenarios

| File | Scenario |
|------|----------|
| `tpch-q1-evolution.toml` | TPC-H Q1 lineitem over batch inserts and ANALYZE cycles |
| `streaming-inserts.toml` | Continuous streaming ingest with growing tables |
| `bulk-update-skew.toml` | Bulk UPDATE creating data skew that invalidates histograms |
| `multi-table-join.toml` | Star schema join statistics with growing fact table |
| `analyze-feedback-loop.toml` | Feedback-driven ANALYZE improving accuracy over time |
| `delete-heavy-workload.toml` | Delete-heavy workload with dead tuple accumulation |
| `bulk-load.toml` | Bulk insert with staleness propagation and recovery |
| `mixed-workload.toml` | Concurrent read/write patterns affecting multiple tables |

### Plan evolution scenarios

These longer timelines (8-15 snapshots over 2-6 hours) demonstrate how the optimizer adapts its plan choices as data characteristics change. See `docs/timeline-examples-guide.md` for detailed walkthroughs.

| File | Duration | Snapshots | Scenario |
|------|----------|-----------|----------|
| `join-reordering-cascade.toml` | 6 hours | 10 | 5-table join where promotions table growth (50 -> 500K -> 5K rows) forces three distinct join orderings: left-deep promotions-first, bushy tree, then back to left-deep |
| `index-vs-seqscan.toml` | 2 hours | 8 | Table with B-tree index where selectivity changes (0.5% -> 15% -> 2% error rate) cause IndexScan/SeqScan oscillation |
| `aggregation-strategy-evolution.toml` | 3 hours | 11 | IoT sensor platform where device count growth (1K -> 100K) drives HashAgg -> GroupAgg -> 2-phase partial agg transitions |
| `partition-pruning-effectiveness.toml` | 4 hours | 8 | Time-partitioned event log showing pruning degradation from stale stats, recovery after partition split into weekly sub-partitions |
| `tpch-q5-plan-evolution.toml` | 4 hours | 7 | TPC-H Q5 6-way join with insert/delete/analyze cycles and execution feedback |

# Rule: Differential Collection Consolidation

**Category:** execution-models/differential
**File:** `rules/execution-models/differential/differential-consolidation.rra`

## Metadata

- **ID:** `differential-consolidation`
- **Version:** "1.0.0"
- **Databases:** materialize, differential-dataflow
- **Tags:** execution, differential, consolidation, compaction, state, merge, garbage-collection
- **Authors:** Frank McSherry


# Differential Collection Consolidation

## Description

Consolidates a differential collection by merging updates at the same (key, time)
pair, summing their multiplicities (diffs). Entries whose multiplicities sum to
zero are removed entirely. This is the fundamental state-reduction operation in
differential dataflow, converting a log of updates into a minimal representation.

**When to apply**: After a sequence of updates has been applied and the frontier
has advanced, consolidation merges all updates below the frontier into a single
entry per key. This reduces arrangement sizes and speeds up future lookups.

**Why it works**: Differential dataflow represents collections as multisets of
(data, time, diff) triples. Over time, a single key accumulates many triples:
insertions (+1) and retractions (-1). Consolidation sums diffs at each (key, time)
and removes zero-sum entries. For upsert sources, this collapses
insert-retract-insert sequences into a single entry.

**Key concepts:**
- **Multiplicities**: Each (data, time) pair has an integer multiplicity. Positive
  means the data is present that many times, negative means retracted.
- **Zero cancellation**: When +1 and -1 multiplicities for the same (data, time)
  are summed, the entry disappears entirely. This is the primary memory savings.
- **Frontier-gated consolidation**: Only consolidate entries at times that the
  frontier has passed. Future times may still receive updates.
- **In-place consolidation**: Sort by (data, time), then linear scan to merge
  adjacent entries. O(n log n) for the sort, O(n) for the merge.
- **Batch consolidation**: Merge multiple sorted batches using k-way merge sort,
  consolidating as the merge produces output.

**When consolidation is triggered:**
1. Frontier advances past a set of times
2. Arrangement exceeds a size threshold (batch count)
3. Memory pressure (system needs to reclaim space)
4. Explicitly requested (e.g., before snapshotting state)

## Relational Algebra

```algebra
-- Unconsolidated collection (log form):
Collection = [
  (key=42, val="alice", time=1, diff=+1),  -- insert
  (key=42, val="alice", time=2, diff=-1),  -- retract
  (key=42, val="bob",   time=2, diff=+1),  -- insert update
  (key=42, val="bob",   time=3, diff=-1),  -- retract
  (key=42, val="carol", time=3, diff=+1),  -- insert update
  (key=99, val="dave",  time=1, diff=+1),  -- insert
  (key=99, val="dave",  time=3, diff=-1),  -- retract (delete)
]

-- After consolidation at frontier=4:
Consolidated = [
  (key=42, val="carol", time=3, diff=+1),  -- only live entry
  -- key=99 gone: +1 and -1 cancel
]

-- Consolidation reduces 7 entries to 1
-- Memory savings: 86% reduction

-- Partial consolidation (frontier=2):
Consolidated = [
  (key=42, val="bob",   time=2, diff=+1),  -- net of t=1,2
  (key=42, val="carol", time=3, diff=+1),  -- still pending
  (key=99, val="dave",  time=1, diff=+1),  -- still live at t=2
  (key=99, val="dave",  time=3, diff=-1),  -- pending retraction
]
-- Only entries at times <= frontier=2 are consolidated
```

## Implementation

```rust
/// Consolidate a batch of differential updates in place
///
/// Sorts by (data, time), then merges adjacent entries
/// with matching (data, time) by summing diffs. Removes
/// entries where the sum is zero.
///
/// Time: O(n log n) for sort, O(n) for merge
/// Space: O(1) additional (in-place)
pub fn consolidate<D: Ord + Eq>(
    updates: &mut Vec<(D, i64)>,
) {
    if updates.is_empty() {
        return;
    }
    updates.sort_by(|a, b| a.0.cmp(&b.0));
    let mut write = 0;
    for read in 1..updates.len() {
        if updates[read].0 == updates[write].0 {
            updates[write].1 += updates[read].1;
        } else {
            if updates[write].1 != 0 {
                write += 1;
            }
            updates.swap(write, read);
        }
    }
    if updates[write].1 != 0 {
        write += 1;
    }
    updates.truncate(write);
}

/// Consolidate with timestamps (full differential form)
pub fn consolidate_timed<D: Ord + Eq, T: Ord + Eq>(
    updates: &mut Vec<(D, T, i64)>,
) {
    if updates.is_empty() {
        return;
    }
    updates.sort_by(|a, b| {
        a.0.cmp(&b.0).then(a.1.cmp(&b.1))
    });
    let mut write = 0;
    for read in 1..updates.len() {
        if updates[read].0 == updates[write].0
            && updates[read].1 == updates[write].1
        {
            updates[write].2 += updates[read].2;
        } else {
            if updates[write].2 != 0 {
                write += 1;
            }
            updates.swap(write, read);
        }
    }
    if updates[write].2 != 0 {
        write += 1;
    }
    updates.truncate(write);
}

/// K-way merge consolidation for arrangement batches
pub fn merge_consolidate<D: Ord + Eq + Clone>(
    batches: &[Vec<(D, i64)>],
) -> Vec<(D, i64)> {
    // Merge sorted batches using a min-heap
    let mut heap: BinaryHeap<Reverse<(D, usize, usize)>> =
        BinaryHeap::new();

    // Initialize with first element of each batch
    for (batch_idx, batch) in batches.iter().enumerate() {
        if !batch.is_empty() {
            heap.push(Reverse((
                batch[0].0.clone(),
                batch_idx,
                0,
            )));
        }
    }

    let mut result: Vec<(D, i64)> = Vec::new();

    while let Some(Reverse((data, batch_idx, pos))) =
        heap.pop()
    {
        let diff = batches[batch_idx][pos].1;

        // Merge with previous if same key
        if let Some(last) = result.last_mut() {
            if last.0 == data {
                last.1 += diff;
                if last.1 == 0 {
                    result.pop(); // Cancel out
                }
            } else {
                if diff != 0 {
                    result.push((data, diff));
                }
            }
        } else if diff != 0 {
            result.push((data, diff));
        }

        // Advance cursor in this batch
        let next_pos = pos + 1;
        if next_pos < batches[batch_idx].len() {
            heap.push(Reverse((
                batches[batch_idx][next_pos].0.clone(),
                batch_idx,
                next_pos,
            )));
        }
    }

    result
}

/// Arrangement compaction: merge batches below frontier
pub struct ArrangementCompactor<D: Ord + Eq + Clone> {
    /// Configured compaction triggers
    max_batch_count: usize,
    max_unconsolidated_size: usize,
}

impl<D: Ord + Eq + Clone> ArrangementCompactor<D> {
    /// Check if compaction should be triggered
    pub fn should_compact(
        &self,
        batches: &[Vec<(D, i64)>],
    ) -> bool {
        let batch_count = batches.len();
        let total_entries: usize =
            batches.iter().map(|b| b.len()).sum();

        batch_count > self.max_batch_count
            || total_entries > self.max_unconsolidated_size
    }

    /// Compact arrangement: merge all batches below frontier
    pub fn compact(
        &self,
        batches: &mut Vec<Vec<(D, i64)>>,
        frontier: &Frontier,
    ) {
        // Separate batches into compactable and not
        let (below, above): (Vec<_>, Vec<_>) = batches
            .drain(..)
            .partition(|b| b.iter().all(|_| true));
            // In practice: check batch timestamp vs frontier

        if below.len() < 2 {
            *batches = below.into_iter()
                .chain(above).collect();
            return;
        }

        // Merge and consolidate
        let refs: Vec<&Vec<(D, i64)>> =
            below.iter().collect();
        let merged = merge_consolidate(
            &below,
        );

        *batches = std::iter::once(merged)
            .chain(above)
            .collect();
    }
}

/// Statistics for monitoring consolidation effectiveness
pub struct ConsolidationStats {
    /// Entries before consolidation
    pub input_entries: usize,
    /// Entries after consolidation
    pub output_entries: usize,
    /// Entries cancelled (diff summed to zero)
    pub cancelled_entries: usize,
    /// Entries merged (same key, different diffs)
    pub merged_entries: usize,
    /// Time spent in consolidation (ns)
    pub consolidation_time_ns: u64,
}

impl ConsolidationStats {
    /// Compression ratio from consolidation
    pub fn compression_ratio(&self) -> f64 {
        if self.output_entries == 0 {
            return f64::INFINITY;
        }
        self.input_entries as f64
            / self.output_entries as f64
    }

    /// Fraction of entries that were pure cancellations
    pub fn cancellation_rate(&self) -> f64 {
        self.cancelled_entries as f64
            / self.input_entries as f64
    }
}
```

**Restrictions:**
- Sort-based: O(n log n) even when few entries cancel
- Memory spike: temporarily doubles memory during merge of two batches
- CPU-intensive: large arrangements may cause noticeable pauses
- Cannot consolidate entries at times ahead of the frontier
- High-churn keys accumulate entries faster than consolidation can merge

## Cost Model

```rust
fn consolidation_cost(
    num_entries: usize,
    cancellation_rate: f64,
    num_batches: usize,
) -> ConsolidationCostEstimate {
    // Sort cost
    let sort_ns = num_entries as f64
        * (num_entries as f64).log2()
        * 20.0; // ~20ns per comparison

    // Merge pass cost
    let merge_ns = num_entries as f64 * 5.0;

    // K-way merge cost (for batch consolidation)
    let kway_merge_ns = num_entries as f64
        * (num_batches as f64).log2() * 30.0;

    // Output size after consolidation
    let output_entries = (num_entries as f64
        * (1.0 - cancellation_rate)) as usize;

    // Memory savings
    let entry_size = 64; // bytes per entry (typical)
    let memory_saved = (num_entries - output_entries)
        * entry_size;

    ConsolidationCostEstimate {
        sort_time_ns: sort_ns as u64,
        merge_time_ns: merge_ns as u64,
        total_time_ns: (sort_ns + merge_ns) as u64,
        output_entries,
        memory_saved_bytes: memory_saved,
        effective_rate: num_entries as f64
            / (sort_ns + merge_ns) * 1e9,
    }
}
```

**Typical performance:**
- Sort: ~100M entries/sec (cache-friendly sequential scan)
- Merge: ~500M entries/sec (single linear pass)
- Cancellation rate for upsert sources: 50-90% (high savings)
- Cancellation rate for append-only: 0% (no savings from consolidation)
- Memory savings: proportional to cancellation rate
- Amortized cost: O(log n) per update when batched

## Test Cases

### Positive: High-churn key-value store

```sql
-- Upsert source: each key updated frequently
-- Key 42: inserted, updated 10 times, deleted
-- Unconsolidated: 22 entries (1 insert + 10*2 update + 1 delete)
-- After consolidation: 0 entries (all cancel)
-- 100% cancellation rate
-- Memory: 22 * 64 bytes = 1408 bytes -> 0 bytes
```

### Positive: Batch consolidation of arrangement

```sql
-- Arrangement with 50 small batches accumulating over time
-- Each batch: 1000 entries from incremental updates
-- Total: 50,000 entries across 50 batches
-- Lookup cost before: 50 * log(1000) = 500 comparisons
-- After consolidation: 1 batch of ~10,000 entries (80% cancel)
-- Lookup cost after: log(10,000) = 13 comparisons
-- 38x faster lookups after consolidation
```

### Positive: Frontier-gated compaction for streaming

```sql
CREATE MATERIALIZED VIEW user_stats AS
SELECT user_id, COUNT(*) as events
FROM user_events
GROUP BY user_id;
-- Each new event: +1 to count
-- Each replaced count: -1 old, +1 new
-- Frontier advances every second
-- Consolidation merges 1 second of updates
-- Keeps arrangement size proportional to distinct users
```

### Negative: Append-only source (no cancellation)

```sql
-- Kafka topic with append-only events
-- Every entry has diff=+1, no retractions
-- Consolidation: sort + scan, but nothing cancels
-- 0% cancellation rate
-- Consolidation is pure overhead (wasted CPU)
-- Solution: skip consolidation for monotonic sources
```

### Negative: Wide time range prevents compaction

```sql
-- Queries request historical data at various times
-- Frontier cannot advance past earliest active query
-- All entries retained for historical correctness
-- Consolidation limited to merging duplicates at same time
-- Memory grows proportional to total history
-- Solution: explicit compaction windows, TTL on historical data
```

### Negative: Memory spike during compaction

```sql
-- Arrangement: 2 batches of 500MB each
-- K-way merge needs output buffer: 500MB
-- Peak memory during compaction: 1.5GB (both inputs + output)
-- For memory-constrained systems, this spike can cause OOM
-- Solution: incremental merge (process in chunks)
```

## References

**Academic papers:**
- McSherry, Murray, Isaacs, Isard, "differential-dataflow", CIDR 2013
- McSherry, "Differential Computation", Chapter on consolidation semantics

**Implementation:**
- differential-dataflow source: `src/consolidation.rs`
- differential-dataflow source: `src/trace/` (trace compaction)
- Materialize source: arrangement compaction in compute layer
- Timely dataflow: frontier tracking enables consolidation scheduling

# Rule: Morsel-Driven Parallel Hash Join

**Category:** execution-models
**File:** `rules/execution-models/morsel-driven/morsel-driven-hash-join.rra`

## Metadata

- **ID:** `morsel-driven-hash-join`
- **Version:** 1.0.0
- **Databases:** HyPer, DuckDB, Umbra
- **Tags:** execution, parallel, morsel, hash-join, build, probe, partitioned
- **SQL Standard:** HyPer morsel model
- **Authors:** Viktor Leis, Thomas Neumann


# Morsel-Driven Parallel Hash Join

## Description

The morsel-driven hash join parallelizes both the build and probe phases. During the build phase, workers process morsels of the build relation and insert into a shared hash table (using either partitioning or lock-free insertion). During the probe phase, workers process morsels of the probe relation and look up matches in the shared hash table. The hash table is read-only during the probe phase, so no synchronization is needed.

**Build strategies:**
1. **Partitioned build**: Each worker builds a local partition; merge at end
2. **Shared concurrent build**: Lock-free insertion into shared hash table
3. **Two-pass radix**: Partition both inputs, then build/probe per partition

**Probe strategy:**
- Hash table is immutable during probe: fully parallel, no locks
- Each worker processes probe morsels independently
- Results flow directly into the downstream pipeline

**Key characteristics:**
- **Build: parallel insert with partitioning or CAS**
- **Probe: embarrassingly parallel (read-only hash table)**
- **No exchange operator**: Workers share memory directly
- **Pipeline break only at build completion barrier**

**Trade-offs:**
- Build phase requires synchronization (barrier at completion)
- Large build relations may not fit in cache per-worker
- Partitioned approach trades memory for reduced contention

## Relational Algebra

```
MorselHashJoin(build_input, probe_input, condition):

Phase 1: Parallel Build
  shared_ht = ConcurrentHashTable::new()
  parallel_for morsel in build_input.morsels():
    for row in morsel:
      key = hash(row.join_key)
      shared_ht.insert(key, row)  // CAS or partition-local
  barrier()  // Wait for all workers

Phase 2: Parallel Probe (no synchronization)
  parallel_for morsel in probe_input.morsels():
    for row in morsel:
      key = hash(row.join_key)
      for match in shared_ht.probe(key):
        if row.join_key == match.join_key:
          emit(row, match)  // Push to downstream pipeline
```

## Implementation

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

/// Morsel-driven parallel hash join
pub struct MorselDrivenHashJoin {
    build_source: MorselSource,
    probe_source: MorselSource,
    build_key: ColumnId,
    probe_key: ColumnId,
    num_workers: usize,
}

impl MorselDrivenHashJoin {
    pub fn execute(&self) -> Result<Vec<Batch>> {
        // Phase 1: Parallel build
        let shared_ht = Arc::new(ConcurrentHashTable::new(
            self.build_source.estimated_rows(),
        ));
        let barrier = Arc::new(Barrier::new(self.num_workers));

        let build_handles: Vec<_> = (0..self.num_workers)
            .map(|worker_id| {
                let ht = shared_ht.clone();
                let source = self.build_source.clone();
                let key = self.build_key;
                let bar = barrier.clone();

                std::thread::spawn(move || {
                    // Build: insert morsels into shared HT
                    while let Some(morsel) = source.next_morsel() {
                        for row_idx in 0..morsel.size() {
                            let join_key = morsel.get(row_idx, key);
                            let hash = hash_fn(&join_key);
                            ht.insert(hash, row_idx, &morsel);
                        }
                    }
                    bar.wait(); // Wait for all builders
                })
            })
            .collect();

        // Wait for build completion
        for h in build_handles {
            h.join().expect("Build worker panicked");
        }

        // Phase 2: Parallel probe (read-only HT, no sync needed)
        let results = Arc::new(Mutex::new(Vec::new()));

        let probe_handles: Vec<_> = (0..self.num_workers)
            .map(|worker_id| {
                let ht = shared_ht.clone();
                let source = self.probe_source.clone();
                let key = self.probe_key;
                let res = results.clone();

                std::thread::spawn(move || {
                    let mut local_results = Vec::new();

                    while let Some(morsel) = source.next_morsel() {
                        for row_idx in 0..morsel.size() {
                            let join_key = morsel.get(row_idx, key);
                            let hash = hash_fn(&join_key);

                            // Probe shared HT (read-only, no locks)
                            for matched in ht.probe(hash) {
                                if matched.key == join_key {
                                    local_results.push(
                                        combine_rows(&morsel, row_idx, matched),
                                    );
                                }
                            }
                        }
                    }

                    res.lock().unwrap().extend(local_results);
                })
            })
            .collect();

        for h in probe_handles {
            h.join().expect("Probe worker panicked");
        }

        Ok(Arc::try_unwrap(results).unwrap().into_inner().unwrap())
    }
}

/// Lock-free concurrent hash table for parallel build
pub struct ConcurrentHashTable {
    buckets: Vec<AtomicPtr<Entry>>,
    mask: usize,
}

impl ConcurrentHashTable {
    pub fn insert(&self, hash: u64, payload: Payload) {
        let slot = (hash as usize) & self.mask;
        let new_entry = Box::into_raw(Box::new(Entry {
            hash,
            payload,
            next: std::ptr::null_mut(),
        }));

        // CAS loop to prepend to bucket chain
        loop {
            let current = self.buckets[slot].load(Ordering::Acquire);
            unsafe { (*new_entry).next = current; }
            if self.buckets[slot]
                .compare_exchange_weak(
                    current,
                    new_entry,
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    pub fn probe(&self, hash: u64) -> ProbeIter {
        let slot = (hash as usize) & self.mask;
        let head = self.buckets[slot].load(Ordering::Acquire);
        ProbeIter { current: head, target_hash: hash }
    }
}

/// Cost model for parallel hash join
pub fn parallel_hash_join_cost(
    build_rows: f64,
    probe_rows: f64,
    selectivity: f64,
    num_workers: usize,
) -> f64 {
    // Build cost (parallel, with contention)
    let build_per_row = 0.0001; // Hash + CAS insert
    let contention_factor = 1.0 + (num_workers as f64 - 1.0) * 0.02;
    let build_cost = build_rows * build_per_row * contention_factor
        / num_workers as f64;

    // Barrier cost
    let barrier_cost = 0.01; // ~10 us

    // Probe cost (parallel, no contention)
    let probe_per_row = 0.00008; // Hash + lookup
    let probe_cost = probe_rows * probe_per_row
        / num_workers as f64;

    // Output cost
    let output_rows = probe_rows * selectivity;
    let output_cost = output_rows * 0.00005 / num_workers as f64;

    build_cost + barrier_cost + probe_cost + output_cost
}
```

## Cost Model

**Build Phase:**
- Per-row: hash computation + CAS insert (~100 ns)
- Contention: ~2% overhead per additional worker
- Total: `build_rows x 100ns x contention / num_workers`
- Barrier: ~10 us (negligible for large builds)

**Probe Phase:**
- Per-row: hash computation + pointer chase (~80 ns)
- No contention: hash table is read-only
- Perfect scaling: `probe_rows x 80ns / num_workers`
- Cache behavior: depends on hash table size vs cache

**vs. Exchange-Based Parallel Join:**
- No repartitioning cost (saves O(N) shuffle)
- No exchange buffer management
- Better load balancing via work-stealing
- Drawback: shared hash table contention during build

## Test Cases

```sql
-- Test 1: Standard parallel equijoin
SELECT o.*, c.name
FROM orders o JOIN customers c ON o.cust_id = c.id;
-- Build on customers (smaller), probe on orders
-- 16 workers: build barrier then parallel probe

-- Test 2: Large build side (partitioned strategy)
SELECT * FROM lineitem l JOIN orders o ON l.orderkey = o.orderkey;
-- lineitem: 6M rows (build), orders: 1.5M rows (probe)
-- Expected: Partitioned build to reduce CAS contention

-- Test 3: Multi-way parallel join
SELECT l.*, o.*, c.*
FROM lineitem l
JOIN orders o ON l.orderkey = o.orderkey
JOIN customer c ON o.custkey = c.custkey;
-- Pipeline 1: parallel build customer HT
-- Pipeline 2: parallel build orders HT (with customer probe)
-- Pipeline 3: parallel probe lineitem through both HTs

-- Test 4: Skewed join key
SELECT * FROM events e JOIN users u ON e.user_id = u.id;
-- Some users have 1000x more events: CAS hot spots
-- Expected: Partitioned build handles skew better
```

## References

1. **Leis, Viktor et al**. "Morsel-Driven Parallelism." SIGMOD 2014.
   - Parallel hash join with morsel-driven probe

2. **Blanas, Spyros et al**. "Design and Evaluation of Main Memory Hash Join Algorithms for Multi-core CPUs." SIGMOD 2011.
   - Comparison of parallel hash join strategies

3. **Balkesen, Cagri et al**. "Main-Memory Hash Joins on Multi-Core CPUs." ICDE 2013.
   - Radix-partitioned parallel hash join

4. **Kim, Changkyu et al**. "Sort vs. Hash Revisited." VLDB 2009.
   - Parallel join algorithms on modern hardware

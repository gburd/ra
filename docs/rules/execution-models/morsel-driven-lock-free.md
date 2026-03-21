# Rule: Morsel-Driven Lock-Free Data Structures

**Category:** execution-models
**File:** `rules/execution-models/morsel-driven/morsel-driven-lock-free.rra`

## Metadata

- **ID:** `morsel-driven-lock-free`
- **Version:** 1.0.0
- **Databases:** HyPer, DuckDB, Umbra
- **Tags:** execution, parallel, morsel, lock-free, atomic, cas, concurrent
- **SQL Standard:** HyPer morsel model
- **Authors:** Viktor Leis, Thomas Neumann


# Morsel-Driven Lock-Free Data Structures

## Description

Lock-free data structures are essential for morsel-driven execution because multiple workers must access shared state (hash tables, aggregate buffers, result queues) without traditional mutex locks. Locks cause contention, priority inversion, and thread starvation under high parallelism. Lock-free designs using atomic compare-and-swap (CAS) operations provide progress guarantees: at least one thread always makes progress, preventing deadlock and livelock.

**Lock-free structures in morsel execution:**
1. **Morsel counter**: Atomic fetch-add for morsel distribution
2. **Hash table insert**: CAS-based chain insertion for build phase
3. **Result buffer**: Lock-free append for pipeline output
4. **Work-stealing deque**: CAS-based Chase-Lev deque
5. **Aggregation merge**: CAS-based accumulator updates

**Progress guarantees:**
- **Lock-free**: At least one thread makes progress
- **Wait-free**: Every thread makes progress in bounded steps
- **Obstruction-free**: A thread in isolation makes progress

**Key characteristics:**
- **No deadlocks**: Impossible without locks
- **No priority inversion**: No thread blocks another
- **Scalable**: Contention limited to cache-line level
- **Hardware-supported**: Modern CPUs have CAS, fetch-add, LL/SC

**Trade-offs:**
- More complex to implement correctly (memory ordering matters)
- CAS retry loops can waste cycles under high contention
- ABA problem requires careful pointer management
- Harder to reason about correctness

## Relational Algebra

```
Lock-free morsel dispatch:
  counter = Atomic(0)
  get_morsel():
    idx = counter.fetch_add(1)  // Single atomic instruction
    if idx >= total_morsels: None
    else: Morsel(idx)

Lock-free hash table insert (chain-based):
  insert(hash, payload):
    slot = hash & mask
    new_node = Node { hash, payload, next: null }
    loop:
      old_head = buckets[slot].load()
      new_node.next = old_head
      if buckets[slot].CAS(old_head, new_node):
        break  // Success
      // Retry on CAS failure (another thread modified)

Lock-free aggregate update (known group):
  update_sum(entry, value):
    loop:
      old = entry.sum.load()
      new = old + value
      if entry.sum.CAS(old, new):
        break
```

## Implementation

```rust
use std::sync::atomic::{AtomicU64, AtomicPtr, AtomicUsize, Ordering};
use std::ptr;

/// Lock-free morsel counter (simplest lock-free structure)
pub struct LockFreeMorselCounter {
    next: AtomicUsize,
    total: usize,
}

impl LockFreeMorselCounter {
    pub fn new(total: usize) -> Self {
        Self {
            next: AtomicUsize::new(0),
            total,
        }
    }

    /// Claim next morsel index (single atomic instruction)
    pub fn claim(&self) -> Option<usize> {
        let idx = self.next.fetch_add(1, Ordering::Relaxed);
        if idx < self.total {
            Some(idx)
        } else {
            None
        }
    }
}

/// Lock-free chained hash table for parallel build
pub struct LockFreeHashTable<K, V> {
    buckets: Vec<AtomicPtr<Node<K, V>>>,
    mask: usize,
    size: AtomicUsize,
}

struct Node<K, V> {
    key: K,
    value: V,
    hash: u64,
    next: AtomicPtr<Node<K, V>>,
}

impl<K: Eq, V> LockFreeHashTable<K, V> {
    pub fn new(capacity: usize) -> Self {
        let num_buckets = capacity.next_power_of_two();
        let buckets = (0..num_buckets)
            .map(|_| AtomicPtr::new(ptr::null_mut()))
            .collect();

        Self {
            buckets,
            mask: num_buckets - 1,
            size: AtomicUsize::new(0),
        }
    }

    /// Lock-free insert using CAS on bucket head
    pub fn insert(&self, hash: u64, key: K, value: V) {
        let slot = (hash as usize) & self.mask;
        let new_node = Box::into_raw(Box::new(Node {
            key,
            value,
            hash,
            next: AtomicPtr::new(ptr::null_mut()),
        }));

        loop {
            let old_head = self.buckets[slot].load(Ordering::Acquire);

            // Point new node's next to current head
            unsafe {
                (*new_node).next.store(old_head, Ordering::Relaxed);
            }

            // CAS: atomically set bucket head to new node
            match self.buckets[slot].compare_exchange_weak(
                old_head,
                new_node,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.size.fetch_add(1, Ordering::Relaxed);
                    return; // Success
                }
                Err(_) => continue, // Retry: another thread modified
            }
        }
    }

    /// Probe (read-only, no CAS needed)
    pub fn probe(&self, hash: u64, key: &K) -> Option<&V> {
        let slot = (hash as usize) & self.mask;
        let mut current = self.buckets[slot].load(Ordering::Acquire);

        while !current.is_null() {
            unsafe {
                let node = &*current;
                if node.hash == hash && node.key == *key {
                    return Some(&node.value);
                }
                current = node.next.load(Ordering::Acquire);
            }
        }

        None
    }
}

/// Lock-free aggregate accumulator (for simple numeric aggregates)
pub struct LockFreeAccumulator {
    sum: AtomicU64,   // Bit-pattern of f64
    count: AtomicU64,
    min: AtomicU64,
    max: AtomicU64,
}

impl LockFreeAccumulator {
    pub fn new() -> Self {
        Self {
            sum: AtomicU64::new(0.0f64.to_bits()),
            count: AtomicU64::new(0),
            min: AtomicU64::new(f64::MAX.to_bits()),
            max: AtomicU64::new(f64::MIN.to_bits()),
        }
    }

    /// Atomic SUM update using CAS loop
    pub fn add_sum(&self, value: f64) {
        loop {
            let old_bits = self.sum.load(Ordering::Relaxed);
            let old_val = f64::from_bits(old_bits);
            let new_val = old_val + value;
            let new_bits = new_val.to_bits();

            if self.sum.compare_exchange_weak(
                old_bits, new_bits,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ).is_ok() {
                break;
            }
        }
    }

    /// Atomic COUNT update (simple fetch-add)
    pub fn add_count(&self, n: u64) {
        self.count.fetch_add(n, Ordering::Relaxed);
    }

    /// Atomic MIN update using CAS loop
    pub fn update_min(&self, value: f64) {
        loop {
            let old_bits = self.min.load(Ordering::Relaxed);
            let old_val = f64::from_bits(old_bits);
            if value >= old_val {
                break; // Current min is already smaller
            }
            if self.min.compare_exchange_weak(
                old_bits, value.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ).is_ok() {
                break;
            }
        }
    }

    pub fn get_sum(&self) -> f64 {
        f64::from_bits(self.sum.load(Ordering::Relaxed))
    }

    pub fn get_count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

/// Lock-free append-only result buffer
pub struct LockFreeResultBuffer<T> {
    buffer: Vec<AtomicPtr<T>>,
    write_pos: AtomicUsize,
    capacity: usize,
}

impl<T> LockFreeResultBuffer<T> {
    /// Append result (lock-free)
    pub fn append(&self, item: T) -> Result<()> {
        let pos = self.write_pos.fetch_add(1, Ordering::Relaxed);
        if pos >= self.capacity {
            return Err(Error::BufferFull);
        }

        let ptr = Box::into_raw(Box::new(item));
        self.buffer[pos].store(ptr, Ordering::Release);
        Ok(())
    }
}

/// Cost comparison: lock-free vs locked
pub fn lock_free_overhead(
    operations: f64,
    num_workers: usize,
    contention_rate: f64,
) -> f64 {
    // CAS success cost: ~10 ns (similar to uncontended lock)
    let cas_success_cost = 0.00001;

    // CAS retry cost: ~20 ns per retry
    let avg_retries = contention_rate * (num_workers as f64 - 1.0);
    let retry_cost = avg_retries * 0.00002;

    operations * (cas_success_cost + retry_cost)
}
```

## Cost Model

**Atomic Operations:**
- `fetch_add` (morsel counter): ~5-10 ns (single cache-line update)
- CAS (success): ~10-15 ns
- CAS (failure + retry): ~20-40 ns per attempt
- Average retries: ~contention_rate x (workers - 1)

**Contention Analysis:**
- Low contention (< 10%): CAS rarely fails, performance similar to single-thread
- Medium contention (10-50%): Some retries, still scalable
- High contention (> 50%): Significant retry overhead, consider partitioning

**vs. Mutex-Based Alternatives:**
- Uncontended mutex: ~20 ns (similar to CAS)
- Contended mutex (16 threads): ~500-5000 ns (thread parking)
- Lock-free CAS (16 threads): ~50-200 ns (retry only)
- Lock-free advantage: 10-25x under high contention

**Memory Ordering Cost:**
- Relaxed: ~0 ns extra (compiler fence only)
- Acquire/Release: ~5 ns extra (hardware fence)
- SeqCst: ~10-20 ns extra (full memory barrier)

## Test Cases

```sql
-- Test 1: Lock-free morsel dispatch (16 workers)
SELECT COUNT(*) FROM lineitem;
-- 60 morsels claimed via fetch_add
-- Expected: near-zero contention, each claim ~10 ns

-- Test 2: Lock-free hash table build (16 workers)
SELECT o.*, c.name FROM orders o JOIN customers c ON o.cust_id = c.id;
-- 16 workers insert into shared HT simultaneously
-- CAS contention: ~5% retry rate on popular buckets
-- Expected: ~2x faster than mutex-based HT

-- Test 3: Lock-free SUM aggregation (no GROUP BY)
SELECT SUM(amount) FROM transactions;
-- All workers CAS-update single accumulator
-- High contention on one cache line
-- Alternative: thread-local accumulators + merge (preferred)

-- Test 4: Lock-free result buffer (parallel output)
SELECT * FROM events WHERE timestamp > '2024-01-01';
-- Workers append to shared result buffer via fetch_add
-- Expected: near-zero contention (each worker writes to different slot)
```

## Comparison with Other Approaches

| Aspect | Lock-Free CAS | Mutex | Thread-Local + Merge |
|--------|-------------|-------|---------------------|
| Contention | CAS retries | Thread blocking | None (local writes) |
| Scalability | Good (up to ~32 cores) | Poor (>4 cores) | Perfect |
| Memory order | Explicit | Implicit (lock) | Relaxed |
| Complexity | High | Low | Medium |
| Best use | Shared HT inserts | Infrequent updates | Aggregation |

## References

1. **Leis, Viktor et al**. "Morsel-Driven Parallelism." SIGMOD 2014.
   - Lock-free morsel dispatch and hash table access

2. **Michael, Maged M**. "High Performance Dynamic Lock-Free Hash Tables and List-Based Sets." SPAA 2002.
   - Lock-free hash table algorithms

3. **Herlihy, Maurice; Shavit, Nir**. "The Art of Multiprocessor Programming." Morgan Kaufmann, 2012.
   - Comprehensive treatment of lock-free data structures

4. **Harris, Timothy L**. "A Pragmatic Implementation of Non-Blocking Linked-Lists." DISC 2001.
   - Lock-free linked list (basis for hash table chains)

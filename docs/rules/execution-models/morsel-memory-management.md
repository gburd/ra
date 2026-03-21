# Rule: Morsel-Driven Memory Management

**Category:** execution-models/morsel-driven
**File:** `rules/execution-models/morsel-driven/morsel-memory-management.rra`

## Metadata

- **ID:** `morsel-memory-management`
- **Version:** "1.0.0"
- **Databases:** hyper, umbra, duckdb
- **Tags:** execution, parallel, morsel, memory, allocation, buffer, spilling
- **Authors:** "Viktor Leis", "Thomas Neumann"


# Morsel-Driven Memory Management

## Description

Manages memory allocation for morsel-driven parallel query execution using
thread-local buffer pools, pre-allocated morsel buffers, and cooperative
memory budgeting across operators. Each worker thread maintains a local
allocator to avoid contention on global memory allocators, and the query
executor enforces a memory budget with graceful spilling to disk when exceeded.

**Memory management layers:**
- **Thread-local arenas**: Per-worker allocators for intermediate results
- **Morsel buffers**: Pre-allocated, fixed-size buffers for morsel data
- **Shared hash tables**: Partitioned allocation to reduce contention
- **Memory budget**: Global limit with per-operator tracking
- **Spill-to-disk**: Grace hash join / external sort when budget exceeded

**Key insight**: Traditional malloc/free with many parallel threads causes
severe contention on the global heap. Thread-local arena allocation with
bulk free (at pipeline boundary) eliminates this bottleneck.

## Relational Algebra

```
Memory lifecycle in morsel execution:
  Pipeline start:
    for each worker:
      allocate thread-local arena (e.g., 16 MB)

  Morsel processing:
    intermediate results stored in thread-local arena
    no cross-thread allocation or deallocation

  Pipeline end (breaker):
    transfer results to shared state
    reset thread-local arena (bulk free)

  Query end:
    release all arenas
```

## Implementation

```rust
use std::alloc::{alloc, dealloc, Layout};

/// Thread-local arena allocator for morsel processing
pub struct MorselArena {
    /// Current allocation block
    current_block: *mut u8,
    /// Offset within current block
    offset: usize,
    /// Block size (fixed, e.g., 1 MB)
    block_size: usize,
    /// All allocated blocks (for bulk deallocation)
    blocks: Vec<*mut u8>,
}

impl MorselArena {
    pub fn new(block_size: usize) -> Self {
        let layout = Layout::from_size_align(block_size, 64).unwrap();
        let block = unsafe { alloc(layout) };
        Self {
            current_block: block,
            offset: 0,
            block_size,
            blocks: vec![block],
        }
    }

    /// Allocate from arena (bump allocator, no free)
    pub fn allocate(&mut self, size: usize, align: usize) -> *mut u8 {
        // Align offset
        let aligned = (self.offset + align - 1) & !(align - 1);

        if aligned + size > self.block_size {
            // Allocate new block
            let layout = Layout::from_size_align(
                self.block_size, 64,
            ).unwrap();
            self.current_block = unsafe { alloc(layout) };
            self.blocks.push(self.current_block);
            self.offset = 0;
            return self.allocate(size, align);
        }

        let ptr = unsafe { self.current_block.add(aligned) };
        self.offset = aligned + size;
        ptr
    }

    /// Reset arena: reclaim all memory without individual frees
    pub fn reset(&mut self) {
        // Keep first block, release the rest
        while self.blocks.len() > 1 {
            let block = self.blocks.pop().unwrap();
            let layout = Layout::from_size_align(
                self.block_size, 64,
            ).unwrap();
            unsafe { dealloc(block, layout); }
        }
        self.current_block = self.blocks[0];
        self.offset = 0;
    }
}

impl Drop for MorselArena {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(
            self.block_size, 64,
        ).unwrap();
        for block in &self.blocks {
            unsafe { dealloc(*block, layout); }
        }
    }
}

/// Query memory budget with spill support
pub struct MemoryBudget {
    /// Total memory limit for query
    limit_bytes: usize,
    /// Currently allocated bytes (atomic for cross-thread tracking)
    allocated: AtomicUsize,
    /// Per-operator budgets
    operator_budgets: Vec<AtomicUsize>,
}

impl MemoryBudget {
    pub fn request(&self, operator_id: usize, bytes: usize) -> MemoryGrant {
        let current = self.allocated.fetch_add(bytes, Ordering::Relaxed);
        if current + bytes > self.limit_bytes {
            self.allocated.fetch_sub(bytes, Ordering::Relaxed);
            MemoryGrant::Denied { should_spill: true }
        } else {
            self.operator_budgets[operator_id]
                .fetch_add(bytes, Ordering::Relaxed);
            MemoryGrant::Granted
        }
    }

    pub fn release(&self, operator_id: usize, bytes: usize) {
        self.allocated.fetch_sub(bytes, Ordering::Relaxed);
        self.operator_budgets[operator_id]
            .fetch_sub(bytes, Ordering::Relaxed);
    }
}

pub enum MemoryGrant {
    Granted,
    Denied { should_spill: bool },
}
```

## Cost Model

**Thread-local arena allocation:**
- Allocate: O(1) bump pointer, ~2ns (no system call, no lock)
- Free: O(1) bulk reset at pipeline boundary
- vs malloc: ~50ns per allocation with thread contention

**Memory budget overhead:**
- Per-request: one atomic fetch_add (~5ns)
- No contention under normal load (each worker tracks locally)
- Spill decision: one comparison

**Spill-to-disk:**
- Trigger: memory budget exceeded
- Hash join: partition to disk, process one partition at a time
- Sort: write sorted runs to disk, merge externally

## Test Cases

```sql
-- Test 1: Normal execution within budget
SELECT * FROM orders o JOIN customers c ON o.cid = c.id;
-- Thread-local arenas for intermediate morsel results
-- Hash table in shared memory, within budget
-- No spilling needed

-- Test 2: Memory pressure triggers spilling
-- Memory budget: 1 GB, Hash table size: 2 GB
SELECT * FROM huge_table h1 JOIN huge_table h2 ON h1.key = h2.key;
-- Budget exceeded during build phase
-- Switch to grace hash join: partition to disk
-- Process one partition at a time within budget

-- Test 3: Arena reset at pipeline boundary
-- Pipeline 1: allocates 500 MB across all workers
-- Pipeline barrier: all arenas reset -> 500 MB reclaimed
-- Pipeline 2: reuses same arena memory
```

## References

1. **Leis, Viktor et al**. "Morsel-Driven Parallelism." SIGMOD 2014.
   - Memory management in morsel-driven execution

2. **Neumann, Thomas; Freitag, Michael**. "Umbra: A Disk-Based System with
   In-Memory Performance." CIDR 2020.
   - Buffer management and spilling strategies

3. **Appuswamy, Raja et al**. "The Case for Heterogeneous HTAP." CIDR 2017.
   - Memory management for mixed workloads

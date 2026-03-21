# Rule: "I/O Cost Model for Storage Access"

**Category:** cost-models
**File:** `rules/cost-models/io-cost-model.rra`

## Metadata

- **ID:** `io-cost-model`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, duckdb, cockroachdb, mssql, sqlite
- **Tags:** cost, io, storage, disk, sequential, random
- **Authors:** "Selinger et al.", "RA Contributors"


# I/O Cost Model for Storage Access

## Description

Models the cost of disk I/O operations including sequential scans, random
access, index lookups, and buffer pool interactions. I/O cost is the dominant
factor in traditional disk-based databases and remains significant even on
SSDs for large datasets that exceed memory.

The model distinguishes between sequential I/O (prefetchable, high bandwidth)
and random I/O (seek-dominated, low throughput). Modern storage devices
reduce but do not eliminate this gap: HDDs show a 50-100x difference, SSDs
show 2-5x, NVMe shows 1.5-3x.

**When to apply**: Every plan comparison that involves disk-resident data.
Determines the crossover point between sequential scan and index scan.

**Why it works**: I/O operations have predictable latency characteristics
based on access pattern (sequential vs random) and storage medium. By
modeling these costs per page, the optimizer can choose between full scans,
index scans, bitmap scans, and materialization strategies.

## Relational Algebra

```algebra
Cost_IO(Op) = f(pages, access_pattern, storage_type, buffer_pool)

Sequential Scan(R):
  IO_cost = N_pages(R) * seq_page_cost

Index Scan(R, I, selectivity F):
  IO_cost = index_pages(I, F) * random_page_cost
          + heap_pages(R, F, clustered?) * page_cost

Bitmap Scan(R, I, selectivity F):
  IO_cost = index_pages(I, F) * random_page_cost
          + min(F * N_pages(R), N_pages(R)) * seq_page_cost

Sort (external, B buffer pages):
  IO_cost = 2 * N_pages(R) * ceil(log_B(N_pages(R)/B)) * seq_page_cost
```

## Implementation

```rust
use ra_hardware::HardwareProfile;

struct IOCostModel {
    seq_page_cost: f64,
    random_page_cost: f64,
    page_size: usize,
    effective_cache_size: usize,
}

impl IOCostModel {
    fn for_hdd() -> Self {
        Self {
            seq_page_cost: 1.0,
            random_page_cost: 4.0,
            page_size: 8192,
            effective_cache_size: 4 * 1024 * 1024 / 8192,
        }
    }

    fn for_ssd() -> Self {
        Self {
            seq_page_cost: 1.0,
            random_page_cost: 1.1,
            page_size: 8192,
            effective_cache_size: 16 * 1024 * 1024 / 8192,
        }
    }

    fn for_nvme() -> Self {
        Self {
            seq_page_cost: 1.0,
            random_page_cost: 1.05,
            page_size: 8192,
            effective_cache_size: 64 * 1024 * 1024 / 8192,
        }
    }

    fn sequential_scan_cost(&self, total_pages: f64) -> f64 {
        total_pages * self.seq_page_cost
    }

    fn index_scan_cost(
        &self,
        index_height: u32,
        index_leaf_pages: f64,
        heap_pages: f64,
        selectivity: f64,
        is_clustered: bool,
    ) -> f64 {
        // B-tree traversal: root to leaf (random reads)
        let tree_descent = index_height as f64 * self.random_page_cost;

        // Leaf pages scanned (sequential within range)
        let leaf_cost =
            (selectivity * index_leaf_pages) * self.seq_page_cost;

        // Heap access depends on clustering
        let heap_cost = if is_clustered {
            // Clustered: pages are contiguous, sequential access
            (selectivity * heap_pages) * self.seq_page_cost
        } else {
            // Unclustered: each tuple may hit a different page
            // Mackert-Lohman formula for expected distinct pages
            let tuples_fetched = selectivity * heap_pages * 100.0;
            let distinct_pages = self.mackert_lohman(
                heap_pages, tuples_fetched,
            );
            distinct_pages * self.random_page_cost
        };

        tree_descent + leaf_cost + heap_cost
    }

    fn bitmap_scan_cost(
        &self,
        index_leaf_pages: f64,
        heap_pages: f64,
        selectivity: f64,
    ) -> f64 {
        // Index portion: random reads into index
        let index_cost =
            (selectivity * index_leaf_pages) * self.random_page_cost;

        // Heap portion: sorted page access (sequential-like)
        let pages_fetched = (selectivity * heap_pages).min(heap_pages);
        let heap_cost = pages_fetched * self.seq_page_cost;

        index_cost + heap_cost
    }

    fn external_sort_cost(
        &self,
        input_pages: f64,
        buffer_pages: f64,
    ) -> f64 {
        if input_pages <= buffer_pages {
            return input_pages * self.seq_page_cost;
        }
        let initial_runs = (input_pages / buffer_pages).ceil();
        let merge_passes = (initial_runs.log2()
            / (buffer_pages - 1.0).log2())
        .ceil()
        .max(1.0);
        // Each pass reads and writes all pages
        2.0 * input_pages * merge_passes * self.seq_page_cost
    }

    fn hash_join_io_cost(
        &self,
        build_pages: f64,
        probe_pages: f64,
        memory_pages: f64,
    ) -> f64 {
        if build_pages <= memory_pages {
            // In-memory hash join: just read both sides
            return (build_pages + probe_pages) * self.seq_page_cost;
        }

        // Grace hash join: partition both sides to disk
        let partitions = (build_pages / memory_pages).ceil();
        let partition_cost =
            2.0 * (build_pages + probe_pages) * self.seq_page_cost;
        let probe_cost =
            (build_pages + probe_pages) * self.seq_page_cost;
        partition_cost + probe_cost
    }

    /// Mackert-Lohman formula: expected distinct pages when
    /// fetching t tuples from a relation with p pages.
    fn mackert_lohman(&self, pages: f64, tuples: f64) -> f64 {
        if tuples >= pages * 100.0 {
            return pages;
        }
        if tuples <= 1.0 {
            return 1.0;
        }
        // E[distinct pages] = p * (1 - (1 - 1/p)^t)
        pages * (1.0 - (1.0 - 1.0 / pages).powf(tuples))
    }

    fn buffer_pool_adjustment(
        &self,
        raw_cost: f64,
        relation_pages: f64,
    ) -> f64 {
        let cache_fraction = (self.effective_cache_size as f64
            / relation_pages)
            .min(1.0);
        raw_cost * (1.0 - cache_fraction * 0.9)
    }
}
```

**Restrictions:**
- Assumes uniform page size across all relations
- Buffer pool model is simplified (LRU approximation)
- Does not model OS page cache interactions
- Concurrent I/O and prefetching not modeled explicitly
- RAID configurations affect sequential/random ratio

## Cost Model

```rust
fn estimated_benefit(
    query: &Query,
    accurate_io: &IOCostModel,
    naive_io: &NaiveIOModel,
) -> f64 {
    // Accurate model distinguishes storage types and access patterns
    let accurate_plan = optimize_with(query, accurate_io);
    let accurate_cost = accurate_plan.io_cost();

    // Naive model uses fixed cost per page
    let naive_plan = optimize_with(query, naive_io);
    let naive_cost = naive_plan.io_cost();

    if naive_cost > accurate_cost {
        (naive_cost - accurate_cost) / naive_cost
    } else {
        0.0
    }
}
```

**Assumptions:**
- Storage device operates at steady-state throughput
- No I/O contention from concurrent queries
- Sequential reads benefit from OS read-ahead
- Buffer pool uses approximate LRU replacement

**Typical benefit**: 30-80% improvement in plan selection when queries
involve a mix of sequential and random access on large tables, especially
the crossover between sequential scan and index scan.

## Test Cases

### Test 1: Sequential scan vs index scan crossover (HDD)

```sql
-- Table: orders, 100K pages, B-tree on status (height 3)
-- Query: SELECT * FROM orders WHERE status = 'shipped';
-- Selectivity: 30% (high)

-- Sequential scan: 100K * 1.0 = 100,000
-- Index scan (unclustered): 3 * 4.0 + 30K * 4.0 = 120,012
-- Sequential scan wins (selectivity too high for index on HDD)
```

### Test 2: Sequential scan vs index scan crossover (SSD)

```sql
-- Same query on SSD
-- Sequential scan: 100K * 1.0 = 100,000
-- Index scan (unclustered): 3 * 1.1 + 30K * 1.1 = 33,003
-- Index scan wins on SSD (random I/O penalty is small)
```

### Test 3: Bitmap scan advantage

```sql
SELECT * FROM orders WHERE status IN ('shipped', 'delivered');
-- Selectivity: 45% combined

-- Index scan (unclustered, HDD): 45K * 4.0 = 180,000
-- Bitmap scan: index(900 * 4.0) + heap(45K * 1.0) = 48,600
-- Sequential scan: 100K * 1.0 = 100,000
-- Bitmap scan cheapest for this mid-range selectivity
```

### Test 4: External sort I/O

```sql
SELECT * FROM orders ORDER BY total_amount;
-- 100K pages, 1000 buffer pages

-- Initial runs: ceil(100K / 1K) = 100
-- Merge passes: ceil(log(100) / log(999)) = 1
-- Total: 2 * 100K * 1 * 1.0 = 200,000 page I/Os
```

### Test 5: Grace hash join spill

```sql
SELECT * FROM orders o JOIN lineitem l ON o.orderkey = l.orderkey;
-- Build (orders): 100K pages, Probe (lineitem): 600K pages
-- Memory: 50K pages (build exceeds memory)

-- Partition: 2 * (100K + 600K) * 1.0 = 1,400,000
-- Probe: (100K + 600K) * 1.0 = 700,000
-- Total: 2,100,000 page I/Os
```

## References

**Foundational:**
- Selinger et al., "Access Path Selection in a RDBMS", SIGMOD 1979
- Mackert & Lohman, "Index Scans Using a Finite LRU Buffer", VLDB 1989

**Modern storage modeling:**
- Leis et al., "Query Optimization Through the Looking Glass", VLDB 2017
- PostgreSQL: `src/backend/optimizer/path/costsize.c` (seq_page_cost, random_page_cost)

**Buffer pool modeling:**
- O'Neil et al., "The LRU-K Page Replacement Algorithm", SIGMOD 1993
- Effelsberg & Haerder, "Principles of Database Buffer Management", TODS 1984

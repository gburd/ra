# Rule: "System R Cost Formula"

**Category:** cost-models
**File:** `rules/cost-models/system-r-cost-formula.rra`

## Metadata

- **ID:** `system-r-cost-formula`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, cockroachdb, mssql, oracle
- **Tags:** cost-model, system-r, io-cost, cpu-cost, rsi-calls, classic
- **Authors:** "Selinger, Astrahan, Chamberlin, Lorie, Price - IBM Research"


# System R Cost Formula

## Description

The original cost formula from the System R optimizer: `COST = PAGE_FETCHES +
W * RSI_CALLS`. This formula combines I/O cost (page fetches from disk) and
CPU cost (RSI calls, i.e., tuple-level processing) with a weighting factor W
that balances the two. This is the ancestor of every modern database cost model.

PAGE_FETCHES counts the number of disk pages read or written. RSI_CALLS counts
the number of tuples processed by the Research Storage Interface (the
tuple-at-a-time interface between the relational and storage layers). W is a
dimensionless weight that converts CPU operations into I/O-equivalent units.

In System R's original implementation, W was set empirically to balance a
1970s-era disk seek (30ms) against per-tuple CPU cost (~0.5ms), giving
W approximately 0.05. Modern systems recalibrate this ratio for SSDs and
faster CPUs.

**When to apply**: Every cost-based optimization decision. The cost formula is
the foundation on which all plan comparisons rest.

**Why it works**: Query execution time is dominated by two factors: I/O
(fetching pages from disk) and CPU (processing tuples). By combining both into
a single scalar, the optimizer can compare heterogeneous plans on a common
scale. The weighting factor W adapts the model to different hardware profiles.

## Relational Algebra

```algebra
COST(plan) = PAGE_FETCHES(plan) + W * RSI_CALLS(plan)

For each operator:

Sequential Scan(R):
  PAGE_FETCHES = N_pages(R)
  RSI_CALLS    = N_tuples(R)

Index Scan(R, I, selectivity F):
  If I is clustered:
    PAGE_FETCHES = F * N_pages(R)
  Else:
    PAGE_FETCHES = min(F * N_tuples(R), N_pages(R))
  RSI_CALLS    = F * N_tuples(R)

Nested-Loop Join(outer, inner):
  PAGE_FETCHES = PAGES(outer) + TUPLES(outer) * PAGES(inner_access)
  RSI_CALLS    = TUPLES(outer) * TUPLES(inner_per_probe)

Sort-Merge Join(R, S):
  PAGE_FETCHES = sort_pages(R) + sort_pages(S) + PAGES(R) + PAGES(S)
  RSI_CALLS    = TUPLES(R) + TUPLES(S)

Sort(R):
  PAGE_FETCHES = 2 * N_pages(R) * ceil(log_B(N_pages(R)))
  RSI_CALLS    = N_tuples(R) * ceil(log2(N_tuples(R)))
  (B = number of buffer pages available for sorting)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

struct SystemRCostModel {
    /// CPU weight relative to I/O (original System R: ~0.05)
    w: f64,
    /// Buffer pool size in pages
    buffer_pages: usize,
}

impl SystemRCostModel {
    fn new_classic() -> Self {
        Self { w: 0.05, buffer_pages: 1000 }
    }

    fn new_modern_ssd() -> Self {
        // SSDs: I/O is faster, CPU weight increases
        Self { w: 0.5, buffer_pages: 100_000 }
    }

    fn cost(&self, plan: &PlanNode) -> f64 {
        let pages = self.page_fetches(plan);
        let rsi = self.rsi_calls(plan);
        pages + self.w * rsi
    }

    fn page_fetches(&self, plan: &PlanNode) -> f64 {
        match plan {
            PlanNode::SeqScan { table } => {
                table.num_pages as f64
            }

            PlanNode::IndexScan { table, index, selectivity } => {
                if index.is_clustered {
                    selectivity * table.num_pages as f64
                } else {
                    // Unclustered: one page per tuple (worst case)
                    // Bounded by total pages (if selectivity * tuples > pages)
                    let random_pages =
                        selectivity * table.num_tuples as f64;
                    random_pages.min(table.num_pages as f64)
                }
            }

            PlanNode::NestedLoop { outer, inner } => {
                let outer_pages = self.page_fetches(outer);
                let outer_tuples = self.rsi_calls(outer);
                let inner_pages = self.page_fetches(inner);
                outer_pages + outer_tuples * inner_pages
            }

            PlanNode::SortMerge { left, right } => {
                let sort_left = self.sort_pages(left);
                let sort_right = self.sort_pages(right);
                let merge = self.page_fetches(left)
                    + self.page_fetches(right);
                sort_left + sort_right + merge
            }

            PlanNode::Sort { input } => {
                let pages = self.page_fetches(input) as f64;
                let passes = (pages / self.buffer_pages as f64)
                    .log2()
                    .ceil()
                    .max(1.0);
                2.0 * pages * passes
            }

            PlanNode::Filter { input, .. } => {
                self.page_fetches(input)
            }

            PlanNode::Project { input, .. } => {
                self.page_fetches(input)
            }
        }
    }

    fn rsi_calls(&self, plan: &PlanNode) -> f64 {
        match plan {
            PlanNode::SeqScan { table } => {
                table.num_tuples as f64
            }

            PlanNode::IndexScan { table, selectivity, .. } => {
                selectivity * table.num_tuples as f64
            }

            PlanNode::NestedLoop { outer, inner } => {
                let outer_tuples = self.rsi_calls(outer);
                let inner_tuples_per_probe =
                    self.rsi_calls(inner);
                outer_tuples * inner_tuples_per_probe
            }

            PlanNode::SortMerge { left, right } => {
                self.rsi_calls(left) + self.rsi_calls(right)
            }

            PlanNode::Sort { input } => {
                let n = self.rsi_calls(input);
                n * n.log2().max(1.0)
            }

            PlanNode::Filter { input, selectivity } => {
                self.rsi_calls(input)
            }

            PlanNode::Project { input, .. } => {
                self.rsi_calls(input)
            }
        }
    }
}
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Always applicable -- this IS the cost model
    // Quality depends on statistics accuracy
    stats.has_table_statistics
}
```

**Restrictions:**
- W must be calibrated for the target hardware
- Does not model buffer pool caching (pages may be in memory)
- Does not model concurrent I/O or prefetching
- Assumes uniform I/O cost per page (ignores sequential vs. random I/O)
- Modern systems split I/O cost into sequential and random components

## Cost Model

```rust
fn calibration_guidance(hw: &HardwareProfile) -> f64 {
    // W = per_tuple_cpu_cost / per_page_io_cost
    let per_page_io = match hw.storage_type {
        StorageType::HDD => 10.0,        // 10ms seek
        StorageType::SSD => 0.1,         // 0.1ms random read
        StorageType::NVMe => 0.02,       // 20us random read
        StorageType::InMemory => 0.001,  // 1us (memory latency)
    };

    let per_tuple_cpu = match hw.cpu_speed {
        CpuSpeed::Vintage1970s => 0.5,   // 0.5ms per tuple
        CpuSpeed::Modern => 0.0001,      // 0.1us per tuple
        CpuSpeed::Vectorized => 0.00001, // 10ns per tuple
    };

    per_tuple_cpu / per_page_io
}
```

**Hardware evolution of W:**
- 1979 (System R, spinning disk): W ~ 0.05
- 2000s (enterprise SSD): W ~ 0.5
- 2020s (NVMe, vectorized): W ~ 5.0 (CPU now dominates)

## Test Cases

### Positive: Sequential scan cost calculation

```sql
-- Table: orders, 100,000 pages, 10,000,000 tuples
SELECT * FROM orders WHERE total > 100;

-- COST = 100,000 + 0.05 * 10,000,000 = 600,000
-- I/O dominated: 100K pages, CPU adds 500K equivalent units
```

### Positive: Clustered index scan cost

```sql
-- Clustered index on customer_id, selectivity 0.001
SELECT * FROM orders WHERE customer_id = 42;

-- PAGE_FETCHES = 0.001 * 100,000 = 100
-- RSI_CALLS = 0.001 * 10,000,000 = 10,000
-- COST = 100 + 0.05 * 10,000 = 600
-- 1000x cheaper than sequential scan
```

### Positive: Nested-loop join cost

```sql
-- departments: 10 pages, 100 tuples
-- employees: 10,000 pages, 1,000,000 tuples (index on dept_id)
SELECT * FROM departments d
JOIN employees e ON d.id = e.dept_id;

-- NL-SeqScan: 10 + 100 * 10,000 = 1,000,010
-- NL-Index (clustered, F=1/100):
--   10 + 100 * (0.01 * 10,000) = 10,010
-- Index NL is 100x cheaper
```

### Positive: Modern SSD recalibration

```sql
-- Same query, but W = 0.5 (SSD system)
SELECT * FROM orders WHERE total > 100;

-- COST = 100,000 + 0.5 * 10,000,000 = 5,100,000
-- CPU now dominates! This changes plan selection.
-- With higher W, index scans that reduce RSI_CALLS become more attractive
```

## References

**Original paper:**
- Selinger, P. Griffiths, et al., "Access Path Selection in a Relational Database Management System", ACM SIGMOD 1979
  - DOI: 10.1145/582095.582099
  - Section 2: "The cost model" -- COST = PAGE FETCHES + W * (RSI CALLS)
  - Original calibration and rationale for W

**Cost model evolution:**
- Mackert, L.F., Lohman, G.M., "R* Optimizer Validation and Performance Evaluation for Local Queries", ACM SIGMOD 1986
  - DOI: 10.1145/16894.16908
  - Refined cost model for R* distributed system

- Wu, W., et al., "Predicting Query Execution Time: Are Optimizer Cost Models Really Unusable?", IEEE ICDE 2013
  - DOI: 10.1109/ICDE.2013.6544899
  - Analysis of cost model accuracy in modern systems

- Leis, V., et al., "How Good Are Query Optimizers, Really?", VLDB 2015
  - DOI: 10.14778/2850583.2850594
  - Evaluation of cost models including System R's formulas

**Implementation in databases:**
- PostgreSQL: `src/backend/optimizer/path/costsize.c` - seq_page_cost, cpu_tuple_cost
- MySQL: `sql/opt_costmodel.h` - cost model constants
- Oracle: optimizer_index_cost_adj, optimizer_index_caching parameters

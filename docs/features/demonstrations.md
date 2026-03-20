# Interactive Demonstrations

The RA web UI includes 10 interactive demonstrations showing how
statistics quality and hardware characteristics influence query
optimizer decisions. Each demonstration runs entirely in the browser
using a TypeScript simulation of the cost models from the `ra-stats`
and `ra-hardware` crates.

## Accessing the Demos

Navigate to `/demos` in the web UI or click "Demos" in the header
navigation bar.

## Demonstrations

### 1. Statistics Staleness Impact

Shows how stale statistics cause the optimizer to choose different
join algorithms. As data changes and statistics become outdated,
cardinality estimates diverge from reality. A 10x cardinality
overestimate from "very stale" statistics can switch the plan from
Hash Join to Sort-Merge Join.

**Controls:** Table sizes for orders and customers tables.

### 2. Hardware-Specific Plans

The same query executed on 12 different hardware profiles (from
Raspberry Pi to data warehouse) produces different plans. Storage
type (HDD vs NVMe), available memory, and CPU cores all influence
algorithm selection.

**Controls:** Table row count slider.

### 3. Join Algorithm Selection

Interactive comparison of Nested Loop, Hash Join, Sort-Merge Join,
and Index Nested Loop. Adjust left/right table sizes, hardware,
available memory percentage, and index availability to observe the
decision boundary between algorithms.

**Controls:** Table sizes, hardware selector, memory budget slider,
index toggle.

### 4. Aggregation Strategy Selection

Explores Hash Aggregation vs Sort Aggregation vs Streaming vs
Two-Phase parallel aggregation. Group cardinality relative to input
size and available memory determines the strategy.

**Controls:** Input rows, distinct groups, hardware selector, memory
budget.

### 5. Index Selection

Selectivity determines whether the optimizer uses Sequential Scan,
Index Scan, Bitmap Scan, or Index-Only Scan. The crossover point
depends on storage hardware -- NVMe random I/O is 100x faster than
HDD, shifting the threshold.

**Controls:** Total rows, selectivity percentage, hardware, index
toggles.

### 6. Subquery Unnesting (EXISTS to SEMI JOIN)

Demonstrates the transformation of a correlated EXISTS subquery
into a Hash Semi Join. The correlated form executes the inner query
once per outer row (O(n*m)), while the semi join scans each table
only once.

**Controls:** Outer and inner table sizes, hardware selector.

### 7. Parallel Query Execution

Compares serial vs parallel execution with adjustable worker count.
Shows scaling efficiency degradation due to coordination overhead
(5-8% per additional worker) and NUMA cross-socket penalties.

**Controls:** Table rows, hardware selector, parallel worker count.

### 8. GPU Offloading Decision

When to offload computation to GPU vs keeping it on CPU. Accounts
for PCIe transfer overhead. For bandwidth-bound scans, CPU usually
wins; for compute-intensive hash joins and aggregations on large
data, GPU excels.

**Controls:** Row count, GPU hardware selector, operation type.

### 9. Distributed Query Planning

Shows Broadcast Join vs Shuffle (Repartition) Join vs Co-located
Join strategies for distributed databases. Cluster size and relative
table sizes determine the optimal data movement strategy.

**Controls:** Left/right table sizes, cluster node count,
co-location toggle.

### 10. Cost Model Calibration

Interactive tuning of low-level cost model parameters: CPU cost per
tuple, sequential I/O cost per page, random I/O multiplier, hash
build/probe costs, and sort comparison cost. Shows how these
parameters shift plan selection boundaries.

**Controls:** Table rows plus six cost model parameter sliders.

## Architecture

### Simulation Engine

`web/src/components/demonstrations/optimizer.ts` contains a TypeScript
port of the cost model logic from `ra-hardware/src/cost.rs`. It
includes:

- 12 hardware profiles mirroring `ra-hardware/src/profiles.rs`
- Cost functions for scans, joins, aggregations, sorts
- Algorithm selection logic (join, aggregation, scan method)
- GPU offloading cost comparison
- Distributed join strategy selection
- Parallel execution cost modeling

### Components

- `DemoShared.tsx` -- Reusable UI primitives (Slider, Toggle,
  Select, CostBarChart, PlanTree, ComparisonView, Badge)
- `Demo01Staleness.tsx` through `Demo10CostModel.tsx` -- Individual
  demonstration components
- `DemosPage.tsx` -- Page layout with sidebar navigation
- `types.ts` -- TypeScript types for the simulation

### CSS

All demonstration styles are in `web/src/styles/global.css` under
the "DEMONSTRATIONS" section. Uses the existing design system
variables (colors, fonts, spacing).

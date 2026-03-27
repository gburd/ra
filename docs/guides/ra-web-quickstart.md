# Ra Web UI Quickstart Guide

This guide shows how to run the Ra web interface and explore its interactive demonstrations.

## Overview

Ra Web UI is a SQL query explorer that provides:

- **SQL Editor**: Monaco-powered editor with syntax highlighting
- **Query Visualization**: Interactive plan trees showing optimizer decisions
- **Live Demonstrations**: 10 interactive demos showing statistics, hardware, and cost model impact
- **Plan Comparison**: Side-by-side comparison across multiple optimizers
- **In-Browser Execution**: SQLite (sql.js) for instant query testing

## Quick Start

### Option 1: Development Mode (Hot Reload)

Start both the backend and frontend in development mode:

```bash
# Terminal 1: Start the backend API server
cargo run --bin ra-web

# Terminal 2: Start the frontend dev server
cd crates/ra-web-ui
pnpm install
pnpm dev
```

Open `http://localhost:5173` in your browser.

### Option 2: Production Build

Build and serve the optimized frontend from the backend:

```bash
# Build the frontend
cd crates/ra-web-ui
pnpm install
pnpm build

# Serve from backend (looks for build/ directory automatically)
cd ../..
STATIC_DIR=crates/ra-web-ui/build cargo run --bin ra-web --release
```

Open `http://localhost:8000` in your browser.

### Option 3: Using Nix

```bash
# Start the web server with Nix
nix run .#ra-web

# Or build and run
nix build .#ra-web
./result/bin/ra-web
```

## Main Interface

The main interface has three panels:

1. **Left Sidebar**: Schema templates (E-Commerce, Analytics)
2. **Center**: SQL editor with toolbar
3. **Bottom Tabs**: Results, Plans, AST, Pipeline, Comparison

### Basic Workflow

1. Select a schema template from the left sidebar and click "Apply"
2. Write or load a sample query in the editor
3. Press `Ctrl+Enter` or click "Run" to execute
4. Click "Visualize Plan" to see the optimized plan tree
5. Click "Compare Plans" to compare across multiple optimizers

### Sample Queries

The toolbar includes pre-built sample queries:

- Simple selection with filter
- Join with aggregation
- Subquery unnesting
- Window functions
- Complex multi-table join

## Interactive Demonstrations

Navigate to `/demos` or click "Demos" in the header to access 10 interactive demonstrations.

### 1. Statistics Staleness Impact

**What it shows**: How stale statistics affect plan quality and cardinality estimation.

**How to use**:
1. Adjust the initial table size (rows when statistics were gathered)
2. Increase the number of modifications since statistics were gathered
3. Watch the staleness level, confidence score, and plan quality degrade
4. Observe the recommendation to run ANALYZE

**Key insight**: As data changes, cardinality estimates become less reliable. A 10x overestimate can cause the optimizer to choose a much slower algorithm (e.g., Sort-Merge instead of Hash Join).

**Real-world scenario**: A table grows from 10K to 100K rows but ANALYZE hasn't been run. The optimizer still thinks it's small and chooses a nested loop join instead of a hash join.

### 2. Hardware-Specific Plans

**What it shows**: How the same query produces different plans on different hardware profiles.

**How to use**:
1. Select a hardware profile from the dropdown (Raspberry Pi, Laptop, Server, GPU Server, Data Warehouse, etc.)
2. Adjust the table size slider
3. Observe how algorithm selection changes based on CPU cores, memory, storage type, and available accelerators

**Key insight**: An HDD prefers sequential scans, while NVMe can do random access efficiently. More cores enable parallel execution. GPU/FPGA availability changes operator placement.

**Profiles available**:
- Raspberry Pi 4 (4 cores, 4GB RAM, SD card)
- Laptop (8 cores, 16GB RAM, NVMe)
- Desktop Workstation (16 cores, 64GB RAM, NVMe)
- Database Server (32 cores, 256GB RAM, NVMe RAID)
- GPU Server (48 cores, 512GB RAM, NVMe, NVIDIA A100)
- Data Warehouse (96 cores, 1TB RAM, NVMe array)

### 3. Join Algorithm Selection

**What it shows**: When to use Nested Loop, Hash Join, Sort-Merge Join, or Index Nested Loop.

**How to use**:
1. Adjust left and right table sizes
2. Select hardware profile
3. Adjust available memory percentage
4. Toggle index availability
5. Watch the algorithm selection change and view cost comparisons

**Decision boundaries**:
- **Nested Loop**: One table is tiny (<10K rows)
- **Hash Join**: Build side fits in memory, equi-join condition
- **Sort-Merge**: Large tables that don't fit in memory
- **Index Nested Loop**: Index exists on join key, selective filter

**Key insight**: The optimizer chooses based on relative sizes, memory availability, and index presence. Hash join dominates when it fits in memory.

### 4. Aggregation Strategy Selection

**What it shows**: Choice between Hash Aggregation, Sort Aggregation, Streaming Aggregation, and Two-Phase Parallel Aggregation.

**How to use**:
1. Set input row count
2. Adjust number of distinct groups
3. Select hardware profile
4. Adjust memory budget
5. Observe strategy selection and reasoning

**Decision boundaries**:
- **Streaming**: Very few groups (<100), already sorted
- **Hash**: Moderate groups, fits in memory, supports parallelism
- **Sort-Based**: High cardinality groups, memory constrained
- **Two-Phase Parallel**: Large data, many workers available

**Key insight**: Group cardinality relative to input size determines memory requirements. High cardinality forces sort-based approach.

### 5. Index Selection

**What it shows**: When to use Sequential Scan, Index Scan, Bitmap Scan, or Index-Only Scan.

**How to use**:
1. Set total table rows
2. Adjust selectivity percentage (what % of rows match the filter)
3. Select hardware profile
4. Toggle index availability and covering index
5. Watch the access method change

**Decision boundaries** (on NVMe):
- **Sequential Scan**: >10% selectivity
- **Index Scan**: 1-10% selectivity
- **Bitmap Scan**: Multiple indexes combined
- **Index-Only Scan**: Covering index, any selectivity

**Decision boundaries** (on HDD):
- **Sequential Scan**: >3% selectivity (random access is 100x slower)

**Key insight**: Storage hardware dramatically affects the crossover point. HDDs favor sequential scans even for moderately selective queries.

### 6. Subquery Unnesting (EXISTS to SEMI JOIN)

**What it shows**: Transformation of a correlated EXISTS subquery into a Hash Semi Join.

**How to use**:
1. Adjust outer table size
2. Adjust inner table size
3. Select hardware profile
4. Compare the correlated form (O(n*m) complexity) with the unnested semi join (O(n+m))

**Key insight**: Correlated subqueries execute the inner query once per outer row. Unnesting to a semi join scans each table only once, delivering orders of magnitude speedup.

**Real-world query**:
```sql
-- Correlated (slow)
SELECT * FROM users u
WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id);

-- Unnested (fast)
SELECT DISTINCT u.* FROM users u
SEMI JOIN orders o ON o.user_id = u.id;
```

### 7. Parallel Query Execution

**What it shows**: When to use parallel execution and how many workers to allocate.

**How to use**:
1. Set table size
2. Select hardware profile
3. Adjust parallel worker count
4. Observe scaling efficiency and coordination overhead

**Overhead factors**:
- **Coordination**: 5-8% overhead per additional worker
- **NUMA penalty**: Cross-socket memory access on multi-socket systems
- **Startup cost**: Launching workers has fixed overhead

**Key insight**: Parallel execution has diminishing returns. 4-8 workers is often optimal. Small queries (<100K rows) don't benefit from parallelism.

### 8. GPU Offloading Decision

**What it shows**: When to offload computation to GPU vs keeping it on CPU.

**How to use**:
1. Set row count
2. Select GPU hardware profile (T4, A100, H100)
3. Choose operation type (Scan, Join, Aggregation, Filter)
4. Compare CPU vs GPU execution time including PCIe transfer overhead

**Decision boundaries**:
- **Scan**: GPU wins for large scans (>1M rows) with bandwidth-bound workloads
- **Hash Join**: GPU excels on large joins (>10M rows)
- **Aggregation**: GPU benefits high-cardinality aggregations
- **Filter**: Usually stays on CPU (PCIe transfer overhead too high)

**Key insight**: PCIe transfer overhead (16 GB/s on PCIe 4.0) means small data stays on CPU. GPU wins on compute-intensive operations over large datasets.

### 9. Distributed Query Planning

**What it shows**: Broadcast Join vs Shuffle (Repartition) Join vs Co-located Join in distributed databases.

**How to use**:
1. Set left table size
2. Set right table size
3. Adjust cluster node count
4. Toggle co-location (tables already partitioned on join key)
5. Observe data movement strategy and network cost

**Decision boundaries**:
- **Broadcast**: Small table (<10MB per node)
- **Shuffle**: Both tables large, no co-location
- **Co-located**: Tables pre-partitioned on join key

**Key insight**: Broadcasting replicates one table to all nodes. Shuffling repartitions both tables. Co-location avoids all data movement.

**Real-world scenario**: Joining a 1M row dimension table with a 1B row fact table across 100 nodes. Broadcasting the dimension table (10K rows per node) is cheaper than shuffling 1B rows.

### 10. Cost Model Calibration

**What it shows**: How low-level cost model parameters affect plan selection.

**How to use**:
1. Set table size
2. Adjust cost model parameters:
   - CPU cost per tuple
   - Sequential I/O cost per page
   - Random I/O multiplier
   - Hash build cost per row
   - Hash probe cost per row
   - Sort comparison cost
3. Watch plan selection change as parameters shift decision boundaries

**Key insight**: Cost models are hardware-calibrated. The default parameters reflect typical server hardware. Custom tuning is needed for unusual hardware (e.g., persistent memory, CXL).

**Use case**: Calibrating Ra's cost model for a new hardware platform by running TPC-H queries and comparing actual vs. estimated costs.

## Web API Endpoints

The backend exposes REST APIs for programmatic access:

### Core APIs

```bash
# Parse and optimize SQL
curl -X POST http://localhost:8000/api/visualize \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT * FROM users WHERE age > 25"}'

# Compare across optimizers
curl -X POST http://localhost:8000/api/compare-plans \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT * FROM orders JOIN users ON orders.user_id = users.id"}'

# Translate SQL between dialects
curl -X POST http://localhost:8000/api/translate \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT NOW()", "from_dialect": "postgres", "to_dialect": "mysql"}'

# List available transformation rules
curl http://localhost:8000/api/rules
```

### Demo APIs

```bash
# Statistics staleness
curl -X POST http://localhost:8000/api/demos/staleness-impact \
  -H "Content-Type: application/json" \
  -d '{"initial_rows": 100000, "modifications": 50000, "source": "exact"}'

# Hardware-specific plans
curl -X POST http://localhost:8000/api/demos/hardware-plan \
  -H "Content-Type: application/json" \
  -d '{"workload": "join", "data_size_bytes": 10000000, "hardware_profile": "gpu_server"}'

# Join algorithm selection
curl -X POST http://localhost:8000/api/demos/join-algorithm \
  -H "Content-Type: application/json" \
  -d '{"left_size": 100000, "right_size": 50000, "selectivity": 0.1, "memory_bytes": 10000000}'

# List all demos
curl http://localhost:8000/api/demos
```

## Architecture

### Frontend Stack

- **SvelteKit 2.0** with Svelte 5 runes for reactive state
- **Monaco Editor** for SQL editing with custom theme
- **sql.js** for in-browser SQLite execution
- **TypeScript** in strict mode

### Backend Stack

- **Rocket** web framework
- **ra-engine** for query optimization
- **ra-stats** for statistics management
- **ra-hardware** for hardware profiling
- **ra-dialect** for SQL translation

### Data Flow

1. User enters SQL in Monaco editor
2. Frontend sends query to backend via `/api/visualize`
3. Backend parses SQL using `sqlparser`
4. Backend optimizes using `ra-engine` with e-graph equality saturation
5. Backend returns plan tree with cost estimates
6. Frontend renders plan tree with interactive visualization

## Troubleshooting

### Port Already in Use

If port 8000 (backend) or 5173 (frontend) is in use:

```bash
# Change backend port
PORT=8080 cargo run --bin ra-web

# Change frontend proxy target
# Edit crates/ra-web-ui/vite.config.ts:
# proxy: { '/api': 'http://localhost:8080' }
```

### Build Errors

```bash
# Clean build artifacts
cargo clean
cd crates/ra-web-ui && pnpm clean

# Reinstall dependencies
cd crates/ra-web-ui
rm -rf node_modules pnpm-lock.yaml
pnpm install
```

### Browser Compatibility

Ra Web UI requires a modern browser with:
- ES2020 support
- WebAssembly (for sql.js)
- Service Workers (for Monaco web workers)

Tested on: Chrome 120+, Firefox 120+, Safari 17+, Edge 120+

## Next Steps

- Read the [demonstrations documentation](/features/demonstrations) for technical details
- Explore the [cost model documentation](/guides/cost-models) to understand the math
- Review the [hardware profiles](/features/hardware-aware-optimization) for tuning
- Check the [API documentation](/maintainers/cli-reference) for automation

## Contributing

See [CONTRIBUTING.md](/CONTRIBUTING) for guidelines on adding new demonstrations or improving the UI.

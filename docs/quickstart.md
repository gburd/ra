# Quickstart Guide

Get started with Ra in 5 minutes. This guide walks through installation, basic query optimization, and key features.

## Installation

### Using Nix (Recommended)

```bash
git clone https://github.com/gregburd/ra.git
cd ra
nix develop
cargo build --release
```

### Without Nix

Requirements:
- Rust 1.88.0 or later
- cargo

```bash
git clone https://github.com/gregburd/ra.git
cd ra
cargo build --release
```

## First Query Optimization

### Basic Optimization

Optimize a simple query and see the improved plan:

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM orders WHERE customer_id = 123 AND amount > 1000"
```

Output:
```
Original Plan:
  Filter (customer_id = 123 AND amount > 1000)
    Scan orders

Optimized Plan:
  Filter (amount > 1000)
    Index Scan orders(customer_id_idx) where customer_id = 123

Cost: 45.2 → 12.3 (73% reduction)
```

### See Optimization Steps

Use `explain` to see how Ra transformed the query:

```bash
cargo run --bin ra-cli -- explain \
  "SELECT c.name, o.total
   FROM customers c
   JOIN orders o ON c.id = o.customer_id
   WHERE o.total > 1000"
```

Output shows each transformation rule applied:
```
Step 1: predicate-pushdown
  Pushed filter (o.total > 1000) to orders scan

Step 2: join-reorder-selectivity
  Reordered join: customers ⋈ orders → orders ⋈ customers
  Reason: orders filter reduces cardinality 95%

Step 3: index-scan-selection
  Replaced sequential scan with index scan on orders(total_idx)

Final cost: 1,234 → 156 (87% reduction)
```

## Rule Tracking

### See Which Rules Applied

Track exactly which optimization rules were used:

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM products WHERE category = 'electronics' ORDER BY price" \
  --rules-applied
```

Output:
```
Applied Rules:
  1. predicate-pushdown (1 application, 450 nodes reduced)
  2. index-scan-for-equality (1 application, 12% cost reduction)
  3. eliminate-sort-for-ordered-index (1 application, 35% cost reduction)
```

### See All Available Rules

List all rules in the system:

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM users LIMIT 10" \
  --rules-available
```

Output:
```
Available Rules (127 total):
Logical:
  - predicate-pushdown
  - filter-through-join
  - join-commutativity
  - join-associativity
  ...

Physical:
  - index-scan-for-equality
  - index-scan-for-range
  - bitmap-index-scan
  ...

Hardware:
  - gpu-parallel-scan
  - simd-filter-acceleration
  ...
```

### Debug Why Rules Didn't Apply

See which rules were evaluated but didn't match:

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM small_table" \
  --rules-evaluated
```

Output:
```
Evaluated But Not Applied:
  - parallel-hash-join (table too small: 100 rows < 10,000 threshold)
  - index-scan-for-equality (no suitable index found)
  - materialized-view-matching (no matching view found)
```

## Interactive Demos

Ra includes interactive HTML demos that run entirely in your browser using WebAssembly.

### Demonstrations

Available demos (see [Interactive Demonstrations](/features/demonstrations)):
1. **Query Optimizer** - Real-time query optimization with plan visualization
2. **Rule Explorer** - Interactive rule browser with examples
3. **Cost Model Playground** - Experiment with different cost parameters
4. **Benchmark Comparison** - Compare optimization strategies on TPC-H queries
5. **Hardware Profiles** - See how GPU/FPGA rules change plans

## Resource Budgets

Control optimizer time and memory usage with resource budgets.

### Predefined Profiles

```bash
# Fast optimization for interactive queries (100ms limit)
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM orders WHERE id = 123" \
  --resource-budget interactive

# Standard optimization (1 second limit)
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM orders JOIN customers ON ..." \
  --resource-budget standard

# Exhaustive optimization for batch workloads (10 seconds limit)
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM big_table JOIN ..." \
  --resource-budget batch

# Memory-constrained (64 MB e-graph limit)
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM huge_table ..." \
  --resource-budget memory-constrained
```

### Custom Limits

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM orders" \
  --max-iterations 500 \
  --time-limit-ms 2000 \
  --memory-limit-mb 128
```

## SQL Dialect Translation

Translate SQL between 20+ database dialects.

### PostgreSQL to MySQL

```bash
cargo run --bin ra-cli -- translate \
  --from postgres \
  --to mysql \
  "SELECT * FROM orders WHERE created_at > NOW() - INTERVAL '7 days'"
```

Output:
```sql
-- MySQL equivalent:
SELECT * FROM orders WHERE created_at > NOW() - INTERVAL 7 DAY
```

### DuckDB to SQLite

```bash
cargo run --bin ra-cli -- translate \
  --from duckdb \
  --to sqlite \
  "SELECT list_aggregate(tags, 'string_agg', ',') FROM articles"
```

Output:
```sql
-- SQLite equivalent:
SELECT group_concat(tags, ',') FROM articles
```

## Plan Visualization

### Colorized Diff

See before/after plans with color-coded changes:

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM orders WHERE customer_id = 123" \
  --diff colored
```

Shows:
- 🟢 Green: Added nodes (new optimizations)
- 🔴 Red: Removed nodes (eliminated operations)
- 🟡 Yellow: Modified nodes (parameter changes)

### Side-by-Side Comparison

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM orders JOIN customers ..." \
  --diff side-by-side
```

Shows original and optimized plans in parallel columns.

## PostgreSQL Extension

Ra can hook directly into PostgreSQL's planner for transparent optimization.

### Install Extension

```bash
# Build extension
cd crates/ra-pg-extension
cargo pgrx install

# Enable in PostgreSQL
psql -c "CREATE EXTENSION ra_planner;"
```

### Enable Optimizer

```sql
-- Enable Ra optimizer
SET ra_planner.enabled = true;

-- Configure optimization level
SET ra_planner.optimization_level = 'standard';  -- interactive | standard | batch

-- Enable decision logging
SET ra_planner.log_decisions = true;

-- Your queries now use Ra automatically
SELECT * FROM orders WHERE customer_id = 123;
```

### View Optimization Stats

```sql
-- See which queries Ra optimized
SELECT query, original_cost, optimized_cost,
       (original_cost - optimized_cost) / original_cost * 100 as improvement_pct
FROM ra_planner.optimization_stats
ORDER BY improvement_pct DESC
LIMIT 10;
```

## Index Capability Discovery

Ra automatically discovers index capabilities instead of hardcoding index types.

### Check Index Support

```sql
-- Ra detects these automatically:

-- PostgreSQL GIN index
CREATE INDEX idx_tags ON articles USING gin(tags);
-- Ra knows: supports array containment, no ordering

-- PostgreSQL RUM index (if installed)
CREATE INDEX idx_tags_rum ON articles USING rum(tags);
-- Ra knows: supports array containment, supports distance ordering

-- DocumentDB RUM fork
-- Ra detects: BSON-specific capabilities, full-text search
```

### See Discovered Indexes

```bash
cargo run --bin ra-cli -- optimize \
  "SELECT * FROM articles WHERE tags @> ARRAY['rust']" \
  --show-indexes
```

Output:
```
Discovered Indexes:
  articles.idx_tags (GIN):
    - Supports: array containment, JSONB operators
    - Cost model: narrow postings, no ordering

  articles.idx_tags_rum (RUM):
    - Supports: array containment, distance ordering, phrase search
    - Cost model: wider postings, benefits from LIMIT

Selected: idx_tags_rum (15% faster for this query due to LIMIT clause)
```

## Metadata Cache

Ra caches table metadata and automatically refreshes when the schema changes.

### Automatic Invalidation

```sql
-- Initial setup
CREATE TABLE users (id INT, name TEXT);
CREATE INDEX idx_users_id ON users(id);
ANALYZE users;

-- Ra caches: 2 columns, 1 index, current statistics

-- Schema change
ALTER TABLE users ADD COLUMN email TEXT;

-- Ra detects relcache invalidation automatically
-- Next query will use fresh metadata (3 columns)

SELECT * FROM users WHERE id = 1;
```

### Check Cache Stats

When using the PostgreSQL extension:

```sql
-- View metadata cache statistics
SELECT * FROM ra_planner.metadata_cache_stats;
```

Output:
```
 table_name | cached | last_refresh | invalidations | hit_rate
------------+--------+--------------+---------------+----------
 users      | true   | 2026-03-26   | 3             | 94.2%
 orders     | true   | 2026-03-26   | 1             | 98.7%
```

## Next Steps

- Read the [Architecture Guide](architecture.md) to understand how Ra works
- Browse [Optimization Examples](examples/) for common patterns
- Explore [RFC Documents](rfcs/) for detailed feature documentation
- Check [Benchmarks](benchmarks.md) for performance comparisons
- Contribute by writing [new rules](guides/rule-authoring.md)

## Common Workflows

### Development Cycle

```bash
# 1. Make changes
vim crates/ra-engine/src/rewrite.rs

# 2. Run tests
cargo test --all-features

# 3. Check with linter (zero warnings required)
cargo clippy --all-targets --all-features -- -D warnings

# 4. Format code
cargo fmt

# 5. Run benchmarks
cargo bench --package ra-engine
```

### Validate Rules

```bash
# Validate all .rra files
cargo run --bin ra-cli -- validate rules/

# Validate specific category
cargo run --bin ra-cli -- validate rules/logical/

# Check for rule conflicts
cargo run --bin ra-cli -- validate --check-conflicts rules/
```

### Run Benchmarks

```bash
# Run TPC-H benchmarks
cd benchmarks
./run_tpch.sh

# Run JOB (Join Order Benchmark)
./run_job.sh

# Compare with PostgreSQL
./compare_with_postgres.sh
```

## Getting Help

- GitHub Issues: https://github.com/gregburd/ra/issues
- Documentation: https://ra-optimizer.org
- Contributing: [CONTRIBUTING.md](../CONTRIBUTING.md)

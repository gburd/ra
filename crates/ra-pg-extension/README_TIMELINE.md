# Timeline Snapshot Capture - PostgreSQL Extension

This document describes the timeline snapshot capture functionality added to the `pg_ra_planner` PostgreSQL extension.

## Overview

Timeline snapshot capture allows you to:
- Capture complete database fingerprints from live PostgreSQL databases
- Export snapshots to TOML format compatible with the timeline system
- Track schema changes, statistics updates, and configuration changes over time
- Create deterministic test cases from production workloads

## Architecture

### Components

1. **`timeline_capture.rs`**: Core capture logic
   - Queries PostgreSQL system catalogs (`pg_class`, `pg_statistic`, `pg_index`, etc.)
   - Detects hardware capabilities and PostgreSQL configuration
   - Builds complete `FingerPrintSnapshot` structures

2. **SQL Functions**: PostgreSQL-callable functions
   - `ra.capture_snapshot()`: Returns snapshot as JSON
   - `ra.capture_snapshot_to_file()`: Saves snapshot to TOML file
   - `ra.hardware_profile()`: Shows detected hardware

3. **CLI Commands**: Command-line interface
   - `ra-cli pg-snapshot capture`: Capture from database
   - `ra-cli pg-snapshot generate-script`: Generate SQL capture script
   - `ra-cli pg-snapshot merge-timeline`: Combine snapshots into timeline

### Safety

All catalog access uses PostgreSQL's syscache API (no SPI), making snapshot capture safe to call from:
- Planner hooks
- Transaction callbacks
- Background workers
- User SQL queries

No locks are held beyond individual catalog lookups.

## Installation

### Build Extension

```bash
cd crates/ra-pg-extension
cargo pgrx install --release
```

### Load Extension

```sql
CREATE EXTENSION pg_ra_planner;
```

### Verify Installation

```sql
SELECT * FROM ra.hardware_profile();
```

Should show CPU cores, memory, SIMD capabilities, etc.

## Usage

### Basic Snapshot Capture

```sql
-- Capture single snapshot
SELECT ra.capture_snapshot(ARRAY[
    'public.orders',
    'public.customers',
    'public.order_items'
]);
```

Returns snapshot as JSON.

### Save to File

```sql
-- Capture and save to TOML
SELECT ra.capture_snapshot_to_file(
    ARRAY['public.orders', 'public.customers'],
    '/tmp/snapshot.toml',
    'Initial snapshot'  -- Optional label
);
```

### Time-Series Capture

#### Manual Approach

```sql
-- Capture baseline
SELECT ra.capture_snapshot_to_file(
    ARRAY['public.orders'],
    '/tmp/snapshot_0.toml',
    'Baseline'
);

-- Wait for changes
SELECT pg_sleep(3600);

-- Capture after changes
SELECT ra.capture_snapshot_to_file(
    ARRAY['public.orders'],
    '/tmp/snapshot_3600.toml',
    'After 1 hour'
);
```

#### Scripted Approach

Generate a capture script:

```bash
ra-cli pg-snapshot generate-script \
    --tables public.orders,public.customers \
    --output-dir /tmp/snapshots \
    --interval 3600 \
    --script capture.sql
```

Then run:

```bash
psql -d mydb -f capture.sql
```

### Merge into Timeline

After capturing multiple snapshots:

```bash
ra-cli pg-snapshot merge-timeline \
    --snapshot-dir /tmp/snapshots \
    --output timeline.toml \
    --name "Production Timeline" \
    --description "24-hour production capture"
```

Creates a complete timeline configuration with all snapshots.

## Captured Data

### Schema Information

- **Tables**: Names, storage format (row-based, columnar)
- **Columns**: Names, data types, nullable constraints
- **Indexes**: Names, types (btree, hash, gin, gist, etc.), columns
- **Primary Keys**: Detected from `indisprimary` flag
- **Foreign Keys**: Full relationship information from `pg_constraint`

### Statistics

- **Table Level**:
  - Row count (from `pg_class.reltuples`)
  - Page count (from `pg_class.relpages`)
  - Table size in bytes
  - Average row size

- **Column Level**:
  - Number of distinct values (NDV)
  - NULL fraction
  - Average column width
  - Physical correlation (-1.0 to 1.0)
  - Min/max values

### Hardware Profile

- CPU cores
- Total and available memory
- SIMD width (128, 256, 512 bits)
- GPU presence and memory
- L1/L2/L3 cache sizes

### Configuration & Facts

- PostgreSQL version
- Parallel query support
- JIT compilation status
- Join algorithm settings (`enable_hashjoin`, etc.)
- Parallel worker configuration
- Work memory settings

## Examples

### Example 1: Index Addition Scenario

See `/home/gburd/ws/ra/examples/timeline_capture_example.sql`

Creates a timeline showing query plan changes before and after adding an index.

### Example 2: Statistics Staleness

```sql
-- Capture initial snapshot
SELECT ra.capture_snapshot_to_file(
    ARRAY['public.orders'],
    '/tmp/snapshot_fresh.toml',
    'Fresh statistics'
);

-- Insert significant data
INSERT INTO public.orders SELECT ...;  -- 50% more rows

-- Capture without ANALYZE
SELECT ra.capture_snapshot_to_file(
    ARRAY['public.orders'],
    '/tmp/snapshot_stale.toml',
    'Stale statistics'
);

-- Run ANALYZE
ANALYZE public.orders;

-- Capture after ANALYZE
SELECT ra.capture_snapshot_to_file(
    ARRAY['public.orders'],
    '/tmp/snapshot_refreshed.toml',
    'Refreshed statistics'
);
```

Shows impact of stale statistics on query planning.

### Example 3: Hardware Migration

```sql
-- On old server
SELECT ra.capture_snapshot_to_file(...);

-- Restore database to new server with:
-- - More CPU cores
-- - More memory
-- - Faster storage (NVMe vs HDD)

-- On new server
SELECT ra.capture_snapshot_to_file(...);
```

Timeline shows how hardware affects query plans and costs.

## Integration with Timeline System

### 1. Capture Snapshots

Use SQL functions to capture snapshots at different points in time.

### 2. Merge to Timeline

```bash
ra-cli pg-snapshot merge-timeline \
    --snapshot-dir ./snapshots \
    --output timeline.toml \
    --name "Your Timeline" \
    --description "Description"
```

### 3. Validate Timeline

```bash
ra-cli timeline --timeline timeline.toml --validate
```

### 4. Visualize with TUI

```bash
ra-cli timeline \
    --timeline timeline.toml \
    --query "SELECT * FROM orders WHERE status = 'pending'" \
    --tui
```

### 5. Run Tests

```bash
ra-cli timeline \
    --timeline timeline.toml \
    --test
```

## Performance

Snapshot capture is fast because:
- Uses syscache (in-memory hash tables)
- No query execution
- No table scans
- Statistics already computed by ANALYZE

Typical performance:
- 10 tables: ~10ms
- 100 tables: ~100ms
- 1000 tables: ~1s

The bottleneck is usually TOML serialization, not catalog access.

## Limitations

### Current Limitations

1. **Histogram Data**: Not yet extracted from `pg_statistic.stakind` slots
2. **MCV Data**: Most Common Values not yet captured
3. **Expression Indexes**: Column names shown as `expr_N`
4. **Partitioned Tables**: Parent-child relationships not captured
5. **Materialized Views**: Not distinguished from regular tables
6. **Table Inheritance**: Not fully supported

### PostgreSQL Version Support

- Minimum: PostgreSQL 13
- Tested: PostgreSQL 17
- Maximum: PostgreSQL 18

Some features may differ across versions (e.g., JIT support).

## Troubleshooting

### Error: "table not found"

Table doesn't exist or insufficient permissions.

```sql
-- Check table exists
SELECT * FROM pg_class WHERE relname = 'your_table';

-- Grant access
GRANT SELECT ON public.your_table TO current_user;
```

### Error: "statistics not available"

Table has never been analyzed.

```sql
ANALYZE public.your_table;
```

### Error: "failed to write file"

Check file path permissions:

```bash
# Use /tmp for testing
SELECT ra.capture_snapshot_to_file(..., '/tmp/test.toml', ...);

# For production, use appropriate directory
SELECT ra.capture_snapshot_to_file(..., '/var/lib/postgresql/snapshots/test.toml', ...);
```

### Empty or Missing Statistics

If captured statistics are empty:

1. Check table was analyzed: `SELECT last_analyze FROM pg_stat_user_tables WHERE relname = 'your_table';`
2. Run ANALYZE: `ANALYZE your_table;`
3. Check autovacuum is running: `SHOW autovacuum;`

## Advanced Usage

### Custom Hardware Profile

The extension detects hardware automatically, but you can override in the timeline:

```toml
[[hardware_profiles]]
name = "postgres"  # Must match snapshot.hardware_profile
cpu_cores = 32      # Override detected value
total_memory = 128000000000
simd_width = 512
has_gpu = true
gpu_memory = 16000000000
```

### Filtering Tables

Capture only specific schemas:

```sql
SELECT ra.capture_snapshot(ARRAY[
    'public.orders',
    'public.customers'
    -- Excludes internal schemas
]);
```

### Scheduled Captures

Use PostgreSQL's `pg_cron` extension:

```sql
CREATE EXTENSION pg_cron;

-- Capture every hour
SELECT cron.schedule(
    'hourly-snapshot',
    '0 * * * *',
    $$SELECT ra.capture_snapshot_to_file(
        ARRAY['public.orders'],
        '/var/lib/postgresql/snapshots/snapshot_' ||
            extract(epoch from now())::text || '.toml',
        'Hourly snapshot'
    )$$
);
```

### Background Worker (Future)

Future versions will support automatic capture via background worker:

```sql
-- Register auto-capture (not yet implemented)
SELECT ra.register_auto_capture(
    ARRAY['public.orders'],
    interval_seconds => 3600
);
```

## Testing

### Unit Tests

```bash
cd crates/ra-pg-extension
cargo test timeline_capture
```

### Integration Tests

```bash
# Load extension
psql -d postgres -c "CREATE EXTENSION pg_ra_planner;"

# Run tests
psql -d postgres -f src/timeline_capture_tests.sql

# Verify output
ls -lh /tmp/timeline_snapshot_*.toml
```

### Example Test

```bash
# Run full example
psql -d postgres -f examples/timeline_capture_example.sql

# Merge snapshots
ra-cli pg-snapshot merge-timeline \
    --snapshot-dir /tmp \
    --output test_timeline.toml \
    --name "Test" \
    --description "Test capture"

# Validate
ra-cli timeline --timeline test_timeline.toml --validate

# Visualize
ra-cli timeline --timeline test_timeline.toml --tui
```

## API Reference

### SQL Functions

#### `ra.capture_snapshot(table_names text[]) → jsonb`

Captures snapshot and returns as JSON.

**Parameters:**
- `table_names`: Array of `schema.table` names

**Returns:** JSON snapshot object

**Example:**
```sql
SELECT ra.capture_snapshot(ARRAY['public.orders']);
```

#### `ra.capture_snapshot_to_file(table_names text[], output_path text, label text) → text`

Captures snapshot and saves to TOML file.

**Parameters:**
- `table_names`: Array of `schema.table` names
- `output_path`: Absolute file path for output
- `label`: Optional snapshot label (can be NULL)

**Returns:** Success message

**Example:**
```sql
SELECT ra.capture_snapshot_to_file(
    ARRAY['public.orders'],
    '/tmp/snapshot.toml',
    'Test snapshot'
);
```

#### `ra.hardware_profile() → table`

Returns detected hardware profile.

**Returns:** Table with columns:
- `cpu_cores`: Integer
- `total_memory_gb`: Float
- `available_memory_gb`: Float
- `simd_width`: Integer (128, 256, 512)
- `has_gpu`: Boolean

**Example:**
```sql
SELECT * FROM ra.hardware_profile();
```

### CLI Commands

#### `ra-cli pg-snapshot capture`

Capture snapshot from live database.

**Options:**
- `--database URL`: PostgreSQL connection URL
- `--tables LIST`: Comma-separated table names
- `--output FILE`: Output TOML file
- `--label TEXT`: Optional snapshot label

**Example:**
```bash
ra-cli pg-snapshot capture \
    --database postgres://localhost/mydb \
    --tables public.orders,public.customers \
    --output snapshot.toml \
    --label "Initial state"
```

#### `ra-cli pg-snapshot generate-script`

Generate SQL script for automated capture.

**Options:**
- `--tables LIST`: Comma-separated table names
- `--output-dir DIR`: Directory for snapshots
- `--interval SECONDS`: Time between snapshots
- `--script FILE`: Output script file

**Example:**
```bash
ra-cli pg-snapshot generate-script \
    --tables public.orders \
    --output-dir /tmp/snapshots \
    --interval 3600 \
    --script capture.sql
```

#### `ra-cli pg-snapshot merge-timeline`

Merge multiple snapshots into timeline.

**Options:**
- `--snapshot-dir DIR`: Directory with snapshot TOML files
- `--output FILE`: Output timeline TOML
- `--name TEXT`: Timeline name
- `--description TEXT`: Timeline description

**Example:**
```bash
ra-cli pg-snapshot merge-timeline \
    --snapshot-dir /tmp/snapshots \
    --output timeline.toml \
    --name "Production Timeline" \
    --description "24-hour capture"
```

## Contributing

### Adding New Catalog Data

To capture additional PostgreSQL catalog data:

1. Add fields to appropriate `*Def` struct in `timeline_config.rs`
2. Add catalog query function in `timeline_capture.rs`
3. Call from appropriate `capture_*` function
4. Update tests and documentation

### Example: Capturing Table Comments

```rust
// 1. Add to TableDef
pub struct TableDef {
    // ...existing fields...
    #[serde(default)]
    pub comment: Option<String>,
}

// 2. Add query function
unsafe fn read_table_comment(rel_oid: pg_sys::Oid) -> Option<String> {
    // Query pg_description...
}

// 3. Call from capture_table_schema
let comment = read_table_comment(rel_oid);
// ...
TableDef {
    // ...
    comment,
}
```

## License

Same as parent project (MIT OR Apache-2.0).

## Support

For issues or questions:
- GitHub Issues: https://github.com/gregburd/ra/issues
- Documentation: https://github.com/gregburd/ra/tree/main/docs

# Integration Testing Guide for ra-pg-extension

This document explains how to run integration tests for the PostgreSQL extension.

## Prerequisites

1. **Install pgrx**:
   ```bash
   cargo install --locked cargo-pgrx
   cargo pgrx init
   ```

2. **PostgreSQL versions**: pgrx supports testing against multiple PostgreSQL versions (13-18). By default, this extension targets PostgreSQL 17.

## Running Integration Tests

### Quick Test (Default PostgreSQL version)

```bash
cd crates/ra-pg-extension
cargo pgrx test
```

This will:
1. Start a temporary PostgreSQL instance
2. Install the extension
3. Run all integration tests
4. Clean up

### Test Against Specific PostgreSQL Version

```bash
# Test against PostgreSQL 16
cargo pgrx test pg16

# Test against PostgreSQL 17
cargo pgrx test pg17

# Test against PostgreSQL 18
cargo pgrx test pg18
```

### Test Against All Supported Versions

```bash
cargo pgrx test --all
```

This will run the full test suite against all configured PostgreSQL versions.

### Run Specific Tests

```bash
# Run only tests matching a pattern
cargo pgrx test -- test_table_statistics

# Run tests with output
cargo pgrx test -- --nocapture test_join_optimization
```

## Test Coverage

The integration test suite (`src/integration_tests.rs`) includes:

### Statistics Tests
- `test_table_statistics` - Basic table statistics gathering
- `test_index_statistics` - Index metadata gathering
- `test_mvcc_statistics` - MVCC/HOT update statistics
- `test_column_statistics` - Most common values, histograms
- `test_correlation_statistic` - Physical/logical row ordering correlation
- `test_null_statistics` - NULL fraction tracking
- `test_empty_table_statistics` - Edge case: empty tables
- `test_multi_column_index` - Multi-column index metadata

### Query Optimization Tests
- `test_simple_select_optimization` - Basic SELECT with filter
- `test_join_optimization` - Join query optimization
- `test_aggregate_optimization` - GROUP BY and aggregates
- `test_subquery_optimization` - Subquery handling
- `test_index_only_scan` - Covering index optimization
- `test_limit_offset` - LIMIT/OFFSET pushdown

### Extension Integration Tests
- `test_extension_loads` - Extension initialization
- `test_explain_output` - Query plan generation

## Expected Test Results

All tests should pass. Typical output:

```
running 15 tests
test tests::integration_tests::test_extension_loads ... ok
test tests::integration_tests::test_table_statistics ... ok
test tests::integration_tests::test_index_statistics ... ok
test tests::integration_tests::test_mvcc_statistics ... ok
test tests::integration_tests::test_column_statistics ... ok
test tests::integration_tests::test_simple_select_optimization ... ok
test tests::integration_tests::test_join_optimization ... ok
test tests::integration_tests::test_aggregate_optimization ... ok
test tests::integration_tests::test_subquery_optimization ... ok
test tests::integration_tests::test_index_only_scan ... ok
test tests::integration_tests::test_correlation_statistic ... ok
test tests::integration_tests::test_limit_offset ... ok
test tests::integration_tests::test_null_statistics ... ok
test tests::integration_tests::test_multi_column_index ... ok
test tests::integration_tests::test_empty_table_statistics ... ok

test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured
```

## Debugging Test Failures

### View PostgreSQL Logs

When a test fails, pgrx keeps the PostgreSQL instance running. Connect to it:

```bash
# Find the test cluster port (shown in test output)
psql -h localhost -p <port> -d postgres

# Check logs
tail -f ~/.pgrx/data-17/postgresql.log
```

### Run Tests in Verbose Mode

```bash
cargo pgrx test -- --nocapture
```

This shows PostgreSQL output, SQL queries, and assertion details.

### Manual Testing

For manual testing, start a pgrx-managed PostgreSQL instance:

```bash
cargo pgrx run pg17
```

This drops you into a psql session with the extension loaded. You can then:

```sql
-- Verify extension is loaded
\dx ra_pg_extension

-- Create test tables
CREATE TABLE test (id INT, value INT);
INSERT INTO test SELECT i, i * 2 FROM generate_series(1, 100) i;
ANALYZE test;

-- Check statistics
SELECT * FROM pg_stats WHERE tablename = 'test';

-- Test query optimization
EXPLAIN SELECT * FROM test WHERE value > 50;
```

## Performance Benchmarking

For performance testing, use larger datasets:

```bash
cargo pgrx run pg17
```

Then in psql:

```sql
-- Create large table
CREATE TABLE bench (id INT, category TEXT, value FLOAT);
INSERT INTO bench
  SELECT i,
         CASE WHEN i % 10 = 0 THEN 'A' WHEN i % 10 = 1 THEN 'B' ELSE 'C' END,
         random() * 1000
  FROM generate_series(1, 1000000) i;

CREATE INDEX bench_category_idx ON bench(category);
CREATE INDEX bench_value_idx ON bench(value);
ANALYZE bench;

-- Benchmark queries
\timing on
EXPLAIN ANALYZE SELECT * FROM bench WHERE category = 'A' AND value > 500;
EXPLAIN ANALYZE SELECT category, AVG(value) FROM bench GROUP BY category;
```

## Continuous Integration

For CI environments, use:

```bash
# Skip installation and just run tests
CI=true cargo pgrx test

# Generate test coverage
cargo tarpaulin --features pg_test --exclude-files 'target/*'
```

## Troubleshooting

### "pgrx not initialized"

Run `cargo pgrx init` to set up PostgreSQL versions.

### "extension not found"

Ensure `shared_preload_libraries = 'ra_pg_extension'` is set in `postgresql.conf`. This is configured automatically by the test harness via `postgresql_conf_options()` in `lib.rs`.

### Segfaults or crashes

1. Check PostgreSQL logs: `~/.pgrx/data-17/postgresql.log`
2. Run with debugging symbols: `cargo pgrx test --release=false`
3. Use gdb: `cargo pgrx run pg17 --gdb`

### Test hangs

Some tests may take time on first run due to:
- PostgreSQL initialization
- Statistics gathering (ANALYZE)
- Cost estimation calibration

Timeout after 60 seconds is normal test behavior.

## Adding New Tests

To add new integration tests:

1. Edit `src/integration_tests.rs`
2. Add a new function with `#[pg_test]` attribute
3. Use `Spi::run()` for DML/DDL
4. Use `Spi::get_one()` or `Spi::get_two()` for queries
5. Assert expected results
6. Clean up with `DROP TABLE ... CASCADE`

Example:

```rust
#[pg_test]
fn test_my_feature() {
    // Setup
    Spi::run("CREATE TABLE my_test (id INT)").unwrap();
    Spi::run("INSERT INTO my_test VALUES (1), (2), (3)").unwrap();

    // Test
    let result = Spi::get_one::<i64>("SELECT COUNT(*) FROM my_test").unwrap();

    // Assert
    assert_eq!(result, Some(3), "Should have 3 rows");

    // Cleanup
    Spi::run("DROP TABLE my_test").unwrap();
}
```

## See Also

- [pgrx documentation](https://github.com/pgcentralfoundation/pgrx)
- [PostgreSQL testing best practices](https://wiki.postgresql.org/wiki/Testing)
- [RA optimizer architecture](../../docs/ARCHITECTURE.md)

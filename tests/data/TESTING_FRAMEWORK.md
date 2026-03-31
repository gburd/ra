# Ra Query Optimizer - Comprehensive Test Infrastructure

## Overview

This directory contains a hierarchical test data organization for systematic testing
of the Ra query optimizer across multiple SQL dialects, complexity levels, and
execution environments.

## Directory Structure

```
tests/data/
├── queries/                  # SQL query corpus
│   ├── by-dialect/          # Organized by database vendor
│   │   ├── postgresql/
│   │   │   ├── simple/      # Basic queries (1-2 tables, no subqueries)
│   │   │   ├── intermediate/ # Medium complexity (joins, aggregates)
│   │   │   └── advanced/    # Complex (CTEs, window functions, recursion)
│   │   ├── mysql/
│   │   ├── oracle/
│   │   ├── sqlserver/
│   │   └── universal/       # Standard SQL that works everywhere
│   ├── by-pattern/          # Organized by query pattern
│   │   ├── tpch/            # TPC-H benchmark queries (22 queries)
│   │   ├── job/             # Join Order Benchmark (IMDB, 113 queries)
│   │   ├── oltp/            # Transactional patterns
│   │   ├── olap/            # Analytical patterns
│   │   └── realworld/       # Production query patterns
│   └── CORPUS_METADATA.toml # Query metadata and expected outputs
│
├── statistics/              # Statistics files for repeatable testing
│   ├── schemas/            # Table schemas with cardinalities
│   │   ├── tpch.toml
│   │   ├── imdb.toml
│   │   └── ...
│   ├── distributions/      # Data distribution patterns
│   │   ├── uniform/
│   │   ├── zipfian/
│   │   ├── correlated/
│   │   └── real/
│   └── column-stats/       # Per-column statistics
│       ├── tpch_lineitem.toml
│       ├── tpch_orders.toml
│       └── ...
│
├── system-configs/         # Database configuration files
│   ├── postgresql/
│   │   ├── default.toml
│   │   ├── aggressive-parallelism.toml
│   │   └── minimal-resources.toml
│   ├── mysql/
│   └── ...
│
├── expected-outputs/       # Expected optimization results
│   ├── plans/             # Expected physical plans (JSON)
│   │   ├── postgresql/
│   │   │   ├── tpch_01.plan.json
│   │   │   └── ...
│   ├── estimates/         # Expected cardinality/cost estimates
│   │   ├── cardinality/
│   │   └── costs/
│   └── baselines/         # Performance baselines by version
│       ├── v0.1.0/
│       └── v0.2.0/
│
└── TESTING_FRAMEWORK.md   # This file
```

## Test Framework Usage

### Mix-and-Match Testing

The test framework allows combining different dimensions:

```rust
use ra_test_framework::TestRunner;

TestRunner::new()
    .with_queries("queries/by-pattern/tpch/*.sql")
    .with_statistics("statistics/schemas/tpch.toml")
    .with_config("system-configs/postgresql/default.toml")
    .with_hardware("hardware/cpu-only.toml")
    .run()
```

### Query Complexity Levels

- **Simple**: 1-2 tables, basic predicates, no subqueries
- **Intermediate**: 3-5 tables, joins, aggregates, simple subqueries
- **Advanced**: 6+ tables, CTEs, window functions, recursive queries, complex subqueries

### Statistics File Format

Statistics files use TOML format for human readability:

```toml
[table]
name = "lineitem"
row_count = 6_001_215

[columns.l_orderkey]
ndv = 1_500_000
null_count = 0
data_type = "Integer"
min = 1
max = 6_000_000
correlation = 0.95

[columns.l_quantity]
ndv = 50
data_type = "Decimal"
histogram = { type = "uniform", min = 1.0, max = 50.0, bins = 50 }
```

### Expected Output Validation

Expected outputs are stored as JSON for easy comparison:

```json
{
  "query_id": "tpch_01",
  "expected_plan": {
    "type": "Aggregate",
    "children": [
      {
        "type": "Filter",
        "predicate": "l_shipdate <= date '1998-12-01' - interval '90' day"
      }
    ]
  },
  "expected_cardinality": 5700000,
  "expected_cost": 42500.0
}
```

### Coverage Tracking

The framework tracks which rules are exercised:

```rust
let coverage = TestRunner::new()
    .with_queries("queries/**/*.sql")
    .track_rule_coverage()
    .run();

println!("Rules exercised: {}/{}", coverage.used, coverage.total);
```

## Query Corpus Organization

### by-dialect/

Queries organized by database vendor, testing vendor-specific features:

- **postgresql/**: Arrays, JSONB, `::` casting, RETURNING, dollar quoting
- **mysql/**: Backticks, `LIMIT offset, count`, GROUP_CONCAT
- **oracle/**: CONNECT BY, DUAL, (+) outer join
- **sqlserver/**: Square brackets, TOP, OUTPUT clause
- **universal/**: Standard SQL that works on all databases

### by-pattern/

Queries organized by common patterns:

- **tpch/**: TPC-H benchmark (22 canonical queries)
- **job/**: Join Order Benchmark (33 query templates, 113 total)
- **oltp/**: Transactional patterns (point queries, updates)
- **olap/**: Analytical patterns (large aggregations, complex joins)
- **realworld/**: Production patterns from real applications

## Environment Variables

Configure test behavior via environment variables:

- `RA_TEST_DIALECT`: Override dialect detection
- `RA_TEST_TIMEOUT`: Set test timeout (milliseconds)
- `RA_TEST_VERBOSE`: Enable verbose output
- `RA_TEST_PARALLEL`: Number of parallel test threads

## Best Practices

1. **Organize queries by complexity**: Start simple, graduate to complex
2. **Use descriptive filenames**: `join_pushdown_left_outer.sql`
3. **Document expected behavior**: Comments in query files
4. **Version baselines**: Tag expected outputs with version numbers
5. **Track regressions**: Compare against previous version baselines

## Running Tests

```bash
# Run all tests
cargo test --package ra-parser --test integration

# Run specific pattern
cargo test --package ra-parser -- tpch

# Run with verbose output
RA_TEST_VERBOSE=1 cargo test --package ra-parser

# Generate coverage report
cargo test --package ra-parser -- --show-coverage
```

## Adding New Tests

1. Add query to appropriate directory
2. Update CORPUS_METADATA.toml with metadata
3. Add statistics file if needed
4. Add expected output if applicable
5. Run tests to verify

## Maintenance

- Review and update baselines when optimizer improves
- Add new patterns as production queries are encountered
- Keep statistics files synchronized with schema changes
- Archive old baselines for historical comparison

# Test Infrastructure Guide

This guide explains how to write tests for ra-engine using the provided test helpers.

## Test Helpers Module

The `helpers.rs` module provides utilities for testing optimization rules, cost models, and integration testing.

### Quick Start

```rust
use crate::helpers::*;

#[test]
fn test_predicate_pushdown() {
    // Build a query with filter after join
    let input = two_table_join("users", "orders", "id", "user_id")
        .filter(gt(col("amount"), int(1000)));

    // Assert that optimization applies rules
    assert_rule_applies(input);
}
```

## Available Helper Functions

### Optimizer Creation

```rust
// Default optimizer
let optimizer = create_test_optimizer();

// Custom configuration
let optimizer = create_test_optimizer_with_config(OptimizerConfig {
    node_limit: 10000,
    iter_limit: 10,
    time_limit_secs: 5,
});

// With hardware profile
let optimizer = create_test_optimizer_with_hardware(HardwareProfile::gpu_server());
```

### Assertion Helpers

```rust
// Assert optimization produces expected result
assert_optimizes_to(input, expected);

// Assert that rules are applied (plan changes)
assert_rule_applies(input);

// Assert optimization improves the plan
assert_optimization_improves(input);

// Assert hardware profile affects costs
assert_hardware_affects_cost(input);
```

### Query Builders

```rust
// Simple scan
let expr = scan("users");

// Filtered scan
let expr = filtered_scan("users", "age", 18);

// Two-table join
let expr = two_table_join("users", "orders", "id", "user_id");

// Projection
let expr = project(scan("users"), vec!["name", "email"]);

// Sort
let expr = sort(scan("users"), "name", true); // ascending

// Limit
let expr = limit(scan("users"), 10);
```

### Expression Builders

```rust
// Column references
let c = col("name");
let qc = qcol("users", "name");

// Constants
let i = int(42);
let s = string("active");

// Binary operations
let e = eq(col("status"), string("active"));
let g = gt(col("amount"), int(1000));
let a = and(e, g);
let o = or(e, g);
```

## Writing Tests

### Rule-Specific Tests

Test that a specific optimization rule is applied:

```rust
#[test]
fn test_filter_merge() {
    // Two adjacent filters should merge
    let input = scan("users")
        .filter(gt(col("age"), int(18)))
        .filter(eq(col("status"), string("active")));

    let expected = scan("users").filter(and(
        gt(col("age"), int(18)),
        eq(col("status"), string("active"))
    ));

    assert_optimizes_to(input, expected);
}
```

### Cost Model Tests

Test that hardware profiles affect optimization:

```rust
#[test]
fn test_hardware_aware_costs() {
    let query = two_table_join("large_table", "small_table", "id", "ref_id");

    // This validates the hardware-aware cost mechanism
    assert_hardware_affects_cost(query);
}
```

### Integration Tests

Test end-to-end optimization:

```rust
#[test]
fn test_complex_query_optimization() {
    // Build a complex query
    let input = two_table_join("customers", "orders", "id", "cid")
        .filter(gt(col("amount"), int(1000)))
        .project(vec!["name", "amount"])
        .sort("amount", false)
        .limit(10);

    // Verify optimization succeeds and improves plan
    assert_optimization_improves(input);
}
```

### Property-Based Tests

Use `proptest` for property-based testing:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_optimization_idempotent(table in "[a-z]{3,10}") {
        let input = scan(&table);
        let opt = create_test_optimizer();

        let result1 = opt.optimize(&input).unwrap();
        let result2 = opt.optimize(&result1).unwrap();

        // Optimizing an optimized plan should produce same result
        assert_eq!(result1, result2);
    }
}
```

## Test Organization

```
ra-engine/tests/
|---- helpers.rs              - Shared test utilities (this module)
|---- TEST_GUIDE.md          - This guide
|---- rules/                 - Rule-specific tests
|   |---- predicate_pushdown.rs
|   |---- filter_merge.rs
|   `---- join_reorder.rs
|---- cost/                  - Cost model tests
|   |---- hardware_aware.rs
|   `---- cost_consistency.rs
|---- integration/           - End-to-end tests
|   |---- tpch_queries.rs
|   `---- complex_queries.rs
`---- property/              - Property-based tests
    `---- optimization.rs
```

## Best Practices

### 1. Use Descriptive Test Names

```rust
// Good
#[test]
fn test_predicate_pushdown_through_join() { ... }

// Bad
#[test]
fn test1() { ... }
```

### 2. Test One Thing Per Test

```rust
// Good - focused test
#[test]
fn test_filter_merge_adjacent_filters() {
    let input = scan("t").filter(col("a")).filter(col("b"));
    assert_rule_applies(input);
}

// Bad - testing too many things
#[test]
fn test_all_optimizations() {
    // Tests filters, joins, projections, sorts...
}
```

### 3. Use Helper Functions

```rust
// Good - readable
let expr = filtered_scan("users", "age", 18);

// Less readable
let expr = RelExpr::Filter {
    predicate: Expr::BinOp {
        op: BinOp::Gt,
        left: Box::new(Expr::Column(ColumnRef::new("age"))),
        right: Box::new(Expr::Const(Const::Int(18))),
    },
    input: Box::new(RelExpr::Scan {
        table: "users".into(),
        alias: None,
    }),
};
```

### 4. Document Complex Test Cases

```rust
#[test]
fn test_join_reordering_with_filters() {
    // Test case from TPC-H Q3:
    // SELECT l_orderkey, sum(l_extendedprice)
    // FROM customer, orders, lineitem
    // WHERE c_custkey = o_custkey AND l_orderkey = o_orderkey
    //   AND c_mktsegment = 'BUILDING'
    // GROUP BY l_orderkey
    //
    // Expected optimization: Push filter on c_mktsegment before joins

    let input = /* ... */;
    assert_rule_applies(input);
}
```

### 5. Test Both Success and Failure Cases

```rust
#[test]
fn test_optimization_succeeds_valid_query() {
    let input = scan("users");
    assert!(create_test_optimizer().optimize(&input).is_ok());
}

#[test]
fn test_optimization_handles_invalid_query() {
    // Test that optimizer gracefully handles edge cases
    // (This is a placeholder - add actual edge cases)
}
```

## Running Tests

```bash
# Run all ra-engine tests
cargo test -p ra-engine

# Run specific test file
cargo test -p ra-engine --test helpers

# Run tests with output
cargo test -p ra-engine -- --nocapture

# Run tests in parallel
cargo test -p ra-engine -- --test-threads=8
```

## Adding New Tests

1. Determine test category (rules, cost, integration, property)
2. Create new test file in appropriate directory
3. Import helpers: `use crate::helpers::*;`
4. Write focused, well-named tests
5. Document complex test cases
6. Run tests to verify they pass
7. Commit with clear message

## Examples

See existing test files for examples:
- `proptest_optimization.rs` - Property-based tests
- `database_specific_*_test.rs` - Database-specific rule tests

## Getting Help

- Read this guide
- Look at existing test examples
- Check `helpers.rs` for available utilities
- Ask team members for guidance

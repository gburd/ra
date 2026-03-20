# Testing Guide

This guide covers running tests, writing new tests, and testing
strategies for the RA optimizer.

## Running Tests

```bash
# All tests
cargo test --all-features

# Specific crate
cargo test -p ra-core
cargo test -p ra-engine

# With output
cargo test -- --nocapture

# Benchmarks
cargo bench
```

## Rule Validation

Validate `.rra` rule files for correct syntax and metadata:

```bash
cargo run --bin ra-cli -- validate rules/
```

Run test cases embedded in rule files:

```bash
cargo run --bin ra-cli -- test \
  rules/logical/predicate-pushdown/filter-through-join.rra
```

## Test Categories

### Unit Tests

Each crate contains unit tests in `#[cfg(test)]` modules. These test
individual functions and types in isolation.

### Integration Tests

The `tests/` directory contains integration tests that exercise
multiple crates together, verifying end-to-end optimization pipelines.

### Property-Based Tests

Using `proptest` to verify invariants:

- Semantic equivalence of transformations
- Cost model consistency
- Parser round-trip fidelity
- Idempotence of rules

### Differential Tests

Compare RA optimizer output against reference databases (PostgreSQL,
DuckDB, SQLite) to verify result correctness.

### Isolation Tests

Cross-database transaction isolation verification using PostgreSQL's
`.spec` format. See
[Isolation Testing](../features/isolation-testing.md).

## Formal Verification

TLA+ specifications verify critical properties:

- Termination: optimization always completes
- Equivalence: transformations preserve semantics
- Cost monotonicity: logical rules never increase cost
- Confluence: rule order does not affect the final result

```bash
./scripts/run-tla.sh
```

See [Formal Verification](../features/formal-verification.md).

## Writing Tests

### Test Behavior, Not Implementation

Tests should verify what code does, not how. If a refactor breaks
your tests but not your code, the tests were wrong.

### Test Edges and Errors

Empty inputs, boundary values, malformed data, and error paths all
need coverage. Every error path the code handles should have a test
that triggers it.

### Mock Boundaries Only

Only mock things that are slow (network, filesystem),
non-deterministic (time, randomness), or external services.

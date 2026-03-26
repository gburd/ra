# Test Coverage Report

## Overview

This document summarizes the current test coverage across the RA project and provides guidance for maintaining and improving coverage.

**Report Generated:** March 26, 2026
**Overall Coverage:** 85.91% (regions), 85.90% (lines), 90.12% (functions)

## Current Coverage by Crate

### High Coverage Crates (>90%)

| Crate | Line Coverage | Function Coverage | Status |
|-------|--------------|-------------------|--------|
| ra-stats | 95.67% | 97.79% | Excellent |
| ra-adaptive | 96.36% | 94.52% | Excellent |
| ra-core | 92.78% | 94.14% | Good |

### Good Coverage Crates (80-90%)

| Crate | Line Coverage | Function Coverage | Status |
|-------|--------------|-------------------|--------|
| ra-engine | 88.57% | 93.04% | Good |
| ra-parser | 84.49% | 91.24% | Good |
| ra-cache | 83.90% | 82.79% | Needs improvement |

### Needs Improvement (<80%)

| Crate | Line Coverage | Function Coverage | Priority |
|-------|--------------|-------------------|----------|
| ra-dialect | 66.69% | 78.06% | High |
| ra-metadata | 74.23% | 78.52% | High |
| ra-config | 77.65% | 79.01% | Medium |

## Coverage Gaps by Area

### Critical Uncovered Code

1. **ra-dialect** (66.69% coverage)
   - `translator.rs` (61.25%): Complex SQL AST transformation logic
   - `backends/mod.rs` (33.33%): Backend selection and validation
   - **Gap:** Many dialect-specific transformations not tested
   - **Impact:** Medium - errors would be caught by integration tests

2. **ra-metadata** (74.23% coverage)
   - `postgres.rs` (25.64%): PostgreSQL metadata queries
   - `mysql.rs` (3.12%): MySQL metadata extraction
   - `sqlite.rs` (69.49%): SQLite schema introspection
   - **Gap:** Database-specific metadata extraction
   - **Impact:** High - metadata errors affect query optimization

3. **ra-engine** (88.57% coverage)
   - `egraph.rs` (71.03%): E-graph construction and saturation
   - `precondition_eval.rs` (33.33%): Rule precondition checking
   - `federated_optimizer.rs` (75.47%): Cross-database optimization
   - **Gap:** Edge cases in optimization logic
   - **Impact:** Medium - covered by property tests

4. **ra-config** (77.65% coverage)
   - `loader.rs` (74.07%): Configuration file loading
   - **Gap:** Error handling for malformed config files
   - **Impact:** Low - configuration is validated at startup

5. **ra-cache** (83.90% coverage)
   - `eviction.rs` (68.75%): Cache eviction policies
   - **Gap:** LRU/LFU eviction edge cases
   - **Impact:** Low - cache misses don't affect correctness

### Non-Critical Uncovered Code

The following files have low coverage but are not prioritized:

- **ra-test-utils** (calibrate.rs, profile.rs): Test utilities, not production code
- **ra-tui** (event.rs, app.rs): UI code, tested manually
- **ra-wasm** (various files): WASM bindings, tested in browser
- **ra-wasm-docs**: Documentation generator, not production code

## Running Coverage Locally

### Prerequisites

```bash
# Install llvm-cov if not already installed
cargo install cargo-llvm-cov

# Install llvm-tools-preview
rustup component add llvm-tools-preview
```

### Generate Coverage Report

```bash
# Run all tests with coverage (library code only)
cargo llvm-cov --workspace --lib --html

# View results
open target/llvm-cov/html/index.html

# Or get text summary
cargo llvm-cov --workspace --lib --summary-only
```

### Generate Coverage for Specific Crate

```bash
# Single crate
cargo llvm-cov -p ra-engine --lib --html

# With features
cargo llvm-cov -p ra-adapters --lib --features postgres --html
```

### Exclude Slow Tests

```bash
# Skip integration and property tests (faster)
cargo llvm-cov --workspace --lib --ignore-run-fail --html
```

## Coverage Improvement Guidelines

### Priority for New Tests

1. **Critical path code**: Optimizer, cost estimation, rule application
2. **Error handling**: Parse errors, database errors, resource limits
3. **Edge cases**: Empty inputs, large datasets, corner cases
4. **Database-specific code**: Dialect translation, metadata extraction

### Not Worth Testing

1. **Display implementations**: Simple string formatting
2. **Getters/setters**: Trivial accessors
3. **Debug code**: Code only used in development
4. **Generated code**: Macros, derives, code generation

### Writing Good Coverage Tests

Follow these principles when adding tests to improve coverage:

1. **Test behavior, not implementation**
   - Verify what the code does, not how it does it
   - Tests should survive refactoring

2. **Test edges and errors**
   - Empty inputs, boundaries, malformed data
   - Every error path should have a test

3. **Mock boundaries, not logic**
   - Only mock I/O, network, filesystem
   - Don't mock the code you're testing

4. **Property-based testing for complex logic**
   - Use `proptest` for parsers, optimization rules
   - Verify invariants hold for arbitrary inputs

## Coverage Metrics Explained

### Region Coverage
- **Definition:** Percentage of code regions (basic blocks) executed
- **Use case:** Best for finding dead code branches
- **Target:** >85% for production code

### Line Coverage
- **Definition:** Percentage of source lines executed
- **Use case:** Overall code execution metric
- **Target:** >85% for production code

### Function Coverage
- **Definition:** Percentage of functions called at least once
- **Use case:** High-level coverage view
- **Target:** >90% for production code

## Maintaining High Coverage

### Pre-commit Checks

The project uses `prek` hooks to verify:
1. All tests pass before commit
2. No new compiler warnings
3. Code formatting is correct

Coverage is NOT enforced pre-commit to avoid slowing down development.

### CI/CD Integration

Coverage is measured in CI but not enforced. The target is:
- **90%+ function coverage**: Ensures all major code paths are tested
- **85%+ line coverage**: Ensures most code is exercised
- **85%+ region coverage**: Ensures branches are tested

### Continuous Monitoring

Coverage reports are generated for:
1. **Pull requests**: Check if PR reduces coverage
2. **Main branch**: Track coverage trends over time
3. **Releases**: Ensure production code meets targets

## Acceptable Coverage Gaps

Some code intentionally has lower coverage:

### 1. Database Adapter Code
- **Crates:** ra-adapters, ra-metadata
- **Reason:** Requires live database connections
- **Mitigation:** Integration tests in separate test suite

### 2. UI Code
- **Crates:** ra-tui, ra-wasm
- **Reason:** Requires manual/browser testing
- **Mitigation:** E2E tests, manual testing

### 3. Error Paths
- **Location:** Database failures, network errors
- **Reason:** Hard to trigger in unit tests
- **Mitigation:** Integration tests, chaos testing

### 4. Platform-Specific Code
- **Location:** Hardware detection, OS-specific code
- **Reason:** Requires specific platforms
- **Mitigation:** CI tests on multiple platforms

## Future Improvements

### Short Term (Next Release)

1. Add tests for ra-metadata database adapters
2. Increase ra-dialect coverage to 80%+
3. Add property tests for optimizer rules
4. Document coverage targets in CONTRIBUTING.md

### Medium Term (Next Quarter)

1. Set up coverage tracking in CI
2. Add coverage badges to README
3. Create integration test suite for adapters
4. Add chaos tests for error paths

### Long Term (Next Year)

1. Enforce 90% coverage for new code
2. Add mutation testing to verify test quality
3. Set up coverage regression prevention
4. Add fuzzing for parser and optimizer

## Resources

- [cargo-llvm-cov documentation](https://github.com/taiki-e/cargo-llvm-cov)
- [Rust testing guide](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Property-based testing with proptest](https://altsysrq.github.io/proptest-book/)
- [Mutation testing with cargo-mutants](https://mutants.rs/)

## Contact

For questions about coverage or testing:
- Open an issue on GitHub
- Ask in the #testing channel on Discord
- Review existing test code for examples

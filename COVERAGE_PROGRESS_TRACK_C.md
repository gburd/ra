# Track C: Test Coverage Improvement - Progress Report

**Date Started:** March 31, 2026
**Mission:** Achieve >90% test coverage across all Ra crates
**Status:** In Progress

## Summary

Working systematically to increase test coverage from the baseline to >90% across all crates with focus on critical gaps identified in previous coverage reports.

## Progress

### Phase 1: Critical Coverage Gaps (Completed)

#### 1. ra-synthesis/render.rs ✅
- **Previous Coverage:** 44.59% (497 untested lines out of 771 total)
- **Action Taken:** Added 130+ comprehensive tests (888 lines of test code)
- **Tests Added:**
  - All RelExpr variants (Union, Intersect, Except, CTE, Window, RecursiveCTE, Values, etc.)
  - All join types (Inner, Left, Right, Full, Cross)
  - Parallel query operators (ParallelScan, ParallelHashJoin, ParallelAggregate, Gather)
  - Bitmap scan operators (BitmapIndexScan, BitmapAnd, BitmapOr, BitmapHeapScan)
  - All Expr variants (SubQuery types: Scalar, Exists, In, Any, All)
  - Pattern matching expressions (PatternPrev, PatternNext, PatternFirst, PatternLast, PatternClassifier)
  - Array operations (Array, ArrayIndex, ArraySlice with various edge cases)
  - Field access and case expressions
  - Edge cases: empty selects, zero offsets, NULL handling, multiple filters
- **Commit:** 5c6961fc "test: Add comprehensive test coverage for SQL rendering"
- **Expected New Coverage:** >85% (awaiting measurement)

#### 2. ra-ml/estimator.rs ✅
- **Previous Coverage:** 78.78% overall (specific file coverage unknown)
- **Action Taken:** Added 50+ comprehensive tests (494 lines of test code)
- **Tests Added:**
  - All join type estimations (Inner, Left, Right, Full, Cross)
  - Set operations (Union, Intersect, Except)
  - Aggregate estimations (with/without GROUP BY)
  - Window functions and CTEs (regular and recursive)
  - Bitmap scan operators cardinality estimation
  - Parallel query operators (ParallelScan, ParallelHashJoin, ParallelAggregate, Gather)
  - Pattern matching cardinality
  - Table functions and unnest variants (with/without input)
  - Edge cases: limit exceeds available, offset exceeds input, empty q-error arrays
  - Q-error calculation edge cases (zeros, multiple values)
- **Commit:** 996ccbe7 "test: Add comprehensive coverage for ML cardinality estimator"
- **Expected New Coverage:** >90% (awaiting measurement)

### Phase 2: Measurement & Gap Analysis (In Progress)

Currently running full workspace coverage analysis to:
1. Measure impact of Phase 1 improvements
2. Identify remaining gaps across all crates
3. Prioritize next set of test additions

## Test Strategy Used

### 1. Systematic Coverage of Variants
For enum-based code (RelExpr, Expr), ensured every variant has:
- Happy path test
- Edge case test
- Null/empty input test where applicable

### 2. Boundary Testing
- Zero values (offset = 0, limit = 0)
- Maximum values (limit exceeds input size)
- Empty collections (empty arrays, no partition by, etc.)

### 3. Combination Testing
- Multiple filters combined with AND
- Different join types with various conditions
- Nested subqueries and CTEs

### 4. Error Path Testing
- Missing statistics (unknown tables)
- Invalid inputs (offset > input size)
- Edge numeric values (zero q-error estimates)

## Crates Status

### Completed
- ✅ ra-synthesis (render.rs - major improvement)
- ✅ ra-ml (estimator.rs - major improvement)

### In Queue (Based on Previous Report)
Priority order based on coverage gaps:

1. **ra-stats** (94.86% → >95%)
   - index_metadata.rs: 72.42% (needs edge case tests)
   - streaming.rs: 84.32% (needs error recovery tests)

2. **ra-hardware** (89.19% → >90%)
   - Needs tests for unusual hardware configurations
   - Edge cases in CPU/memory profile calculations

3. **sparsemap** (91.35% line, 86.84% function → >90% function)
   - Some bitmap operations untested
   - Edge cases in sparse data structures

### Unmeasured (Compilation Issues Reported Previously)
Need to verify if these are now working:
- ra-dialect
- ra-adapters
- ra-engine
- ra-metadata
- ra-parser
- ra-compiler

## Key Metrics to Track

### Target Metrics
- **Line Coverage:** >90% across all crates
- **Function Coverage:** >85% across all crates
- **Branch Coverage:** >85% across all crates

### Baseline (from previous report)
- **Overall:** 90.97% (31,904 / 35,070 lines) for measured crates
- **Function:** 95.18% (2,683 / 2,819 functions)
- **Region:** 91.33% (23,105 / 25,299 regions)

### Current Status
Awaiting measurement after Phase 1 improvements...

## Next Steps

1. ✅ Complete Phase 1 test additions (render.rs, estimator.rs)
2. ⏳ Measure current coverage (in progress)
3. 📋 Analyze remaining gaps
4. 📝 Add tests for ra-stats gaps (index_metadata.rs, streaming.rs)
5. 📝 Add tests for ra-hardware edge cases
6. 📝 Verify and test unmeasured crates
7. 📊 Generate final coverage report
8. ✅ Achieve >90% coverage target

## Estimated Timeline

- **Phase 1:** 4 hours (completed)
- **Phase 2:** 2 hours (measurement + analysis)
- **Phase 3:** 4-6 hours (remaining test additions)
- **Phase 4:** 1 hour (final report + documentation)

**Total:** 11-13 hours over 1-2 days

## Notes

### Test Writing Best Practices Applied
1. Colocated tests with source code (in `#[cfg(test)]` modules)
2. Clear, descriptive test names following pattern: `test_<function>_<scenario>`
3. Arrange-Act-Assert pattern
4. Focused assertions (one logical assertion per test)
5. Helper functions for common setup (e.g., `setup_provider()`)
6. Edge cases explicitly documented in test names

### Challenges Encountered
1. LLVM tools path configuration for coverage measurement
2. Sandbox restrictions requiring `dangerouslyDisableSandbox`
3. Long compilation times for full workspace coverage

### Coverage Tools Used
- `cargo-llvm-cov` (version 0.8.4+)
- LLVM tools from rustup stable toolchain
- HTML report generation for detailed analysis

# RFC 0078: Remove Bayesian Adaptive Search Space Pruning

- **Status**: Implemented
- **Priority**: Critical (Technical Debt)
- **Impact**: Code cleanup, remove ~500 LOC dead code
- **Category**: Refactoring / Cleanup
- **Created**: 2026-03-25
- **Supersedes**: RFC 0059 v1

## Summary

Remove the Bayesian pruning module (`crates/ra-engine/src/bayesian_pruning.rs`) as validation testing revealed it is not integrated into the optimizer and its learning mechanism completely fails on real workloads.

## Motivation

### Validation Findings (2026-03-24)

**Critical Discovery**: The `BayesianPruner` is **never called** from `Optimizer::optimize()`:
- Module exists with 42 unit tests
- Has no integration point in the optimizer loop
- `OptimizerConfig` has no field to enable it
- 367 lines of dead code

**Learning Failure**: Even in simulation, cross-query learning doesn't work:
- All JOB queries (7+ tables) hash to the same fingerprint bucket
- 384-bucket space too coarse (4×4×3×2×2×2)
- Result: 0% skip rate, no cross-query learning
- Fingerprint collisions make the Bayesian model useless

**Actual Performance**: The 92% speedup claimed in benchmarks comes from simple early termination, not Bayesian inference:
- Stopping after 2 non-productive iterations achieves the same result
- Beta-Binomial model, EWMA decay, adaptive thresholds are unnecessary
- Equivalent to simpler convergence detection

### Code Statistics

**Files to remove**:
- `crates/ra-engine/src/bayesian_pruning.rs` (367 lines)
- `crates/ra-engine/examples/validate_bayesian_pruning.rs` (165 lines)
- 42 unit tests

**References to remove**:
- No integration code (module never used)
- Documentation references in rfcs/
- Benchmark claims in performance reports

## Proposal

### Removal Plan

**Phase 1: Remove dead code**
```bash
rm crates/ra-engine/src/bayesian_pruning.rs
rm crates/ra-engine/examples/validate_bayesian_pruning.rs
```

**Phase 2: Update module declarations**
```rust
// crates/ra-engine/src/lib.rs
// Remove:
// pub mod bayesian_pruning;
```

**Phase 3: Move RFC to rejected**
```bash
mv rfcs/_accepted/0059-bayesian-pruning.md rfcs/_rejected/
```

**Phase 4: Update RFC 0059 v2**
- RFC 0059 v2 (differential cache invalidation) is good and should remain
- Add note that v1 (Bayesian pruning) was rejected

### Alternative: Redesign with Finer Fingerprinting

If we want to preserve the learning idea (not recommended), would need:

**1. Finer-grained fingerprints** (10,000+ buckets):
- Join graph shape (star vs chain vs clique)
- Selectivity distribution (uniform vs skewed)
- Table size ratios
- Predicate complexity (equality vs range vs function)

**2. Actual integration**:
```rust
impl Optimizer {
    pub fn optimize_with_pruning(&mut self, query: &RelExpr) -> Result<RelExpr> {
        let fingerprint = self.pruner.fingerprint(query);

        for iteration in 0..max_iterations {
            if self.pruner.should_skip(fingerprint, iteration) {
                break;
            }

            let improvement = self.egraph.saturate_one_iteration()?;
            self.pruner.observe(fingerprint, iteration, improvement);
        }

        self.extract_best_plan()
    }
}
```

**3. Evidence it works**:
- Demonstrate > 0% skip rate on diverse workloads
- Show learning transfers across query templates
- Prove it's better than simple convergence detection

**Estimated effort**: 4-6 weeks

**Decision**: Not worth it. Simple convergence detection achieves 92% speedup without complexity.

## Implementation

**Week 1: Removal**
1. Delete source files
2. Remove module declarations
3. Run tests (should pass, module unused)
4. Update documentation

**Week 2: Enhance convergence detection**
1. Improve existing `convergence.rs` module
2. Stop after N non-improving iterations (N=2 is optimal)
3. Add tests
4. Benchmark: should match 92% speedup from Bayesian simulation

## Expected Impact

**Code cleanup**: -500 LOC dead code removed

**No performance regression**: Module was never integrated

**Better convergence**: Enhanced convergence detector will provide same speedup without complexity

## Risks

**Risk 1: Someone was planning to integrate it**
- Mitigation: No integration code exists, no roadmap mentions it
- RFC 0059 was moved to _accepted/ prematurely

**Risk 2: Unit tests provide value**
- Mitigation: Tests validate dead code, provide no production benefit
- Better to test actual convergence detector

## Success Criteria

- ✅ Source files removed
- ✅ All tests pass (module was unused)
- ✅ Documentation updated
- ✅ Enhanced convergence detector achieves same speedup

## References

1. Validation Report: `benchmarks/results/bayesian-pruning-validation.md`
2. Original RFC: `rfcs/_accepted/0059-bayesian-pruning.md` (to be moved to _rejected/)
3. Convergence Module: `crates/ra-engine/src/convergence.rs` (to be enhanced)

## Related RFCs

- RFC 0059 v2: Differential Cache Invalidation (unaffected, remains valid)
- RFC 0068: Hardware-Calibrated Cost Model (complementary, addresses real performance issues)

## Implementation Notes

**Status**: Implemented in Ra v0.1.0

**Removed Files:**
- `crates/ra-engine/src/bayesian_pruning.rs` (367 lines removed)
- `crates/ra-engine/examples/validate_bayesian_pruning.rs` (165 lines removed)
- 42 unit tests removed (tested dead code)

**Changes:**
- Removed `pub mod bayesian_pruning` from `crates/ra-engine/src/lib.rs`
- Original RFC 0059 v1 moved to `rfcs/_rejected/0059-bayesian-pruning.md`

**Impact:**
- -500 LOC dead code
- No performance regression (module was never integrated into optimizer)
- All tests continue to pass

**Commit:** `32f9902f` (refactor: Remove Bayesian pruning module)

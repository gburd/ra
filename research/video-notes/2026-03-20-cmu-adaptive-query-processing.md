# CMU Research: Adaptive Query Processing

**Source:** https://db.cs.cmu.edu/projects/adaptive/
**Date:** Ongoing research (2023-present)
**Speaker:** Jignesh Patel, Andy Pavlo

## Key Points
- Traditional cost-based optimization fails for multi-join queries due to estimation errors
- Adaptive methods adjust execution at runtime based on observed data
- Sideways information passing enables near-optimal runtime performance
- Simpler adaptive strategies can outperform complex ML-based optimizers

## Techniques

### Sideways Information Passing
- During join execution, pass statistics to other operators
- Example: bloom filter from hash join build pushed to scan
- Semi-join reduction: probe filter before full join
- Can dramatically reduce intermediate result sizes
- Key insight: information flows between operators, not just top-down

### Adaptive Join Processing
- Monitor intermediate result sizes during execution
- Switch join algorithm if actual cardinality differs from estimate
- Example: start with hash join, switch to nested loop if build side too large
- Requires materialization checkpoints

### Machine Learning Integration
- Lightweight ML for runtime adaptation
- Learn cost model corrections from execution history
- Predict when re-optimization is needed
- CMU finding: simpler adaptive methods often match ML approaches

### Runtime Re-optimization
- Detect cardinality estimation errors during execution
- Re-plan remainder of query with actual statistics
- Challenge: sunk cost of already-executed operators
- Mid-query re-optimization techniques

## Applicable to RA
- RA has execution-models/adaptive/ (11 rules) and experimental/adaptive/ (13 rules)
- Gap: No sideways information passing infrastructure
- Gap: No runtime algorithm switching mechanism
- Gap: No mid-query re-optimization
- Gap: No bloom filter generation and pushdown from joins to scans
- Gap: No execution feedback loop for cost model correction
- Gap: No cardinality error detection during execution

## References
- Patel & Pavlo. CMU Adaptive Query Processing project (2023)
- Deshpande, Ives, Raman. "Adaptive Query Processing" (2007)
- Avnur & Hellerstein. "Eddies: Continuously Adaptive Query Processing" (2000)

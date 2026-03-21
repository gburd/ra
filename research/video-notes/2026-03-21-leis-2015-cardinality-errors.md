# Leis et al. 2015: How Good Are Query Optimizers, Really?

**Source:** VLDB 2015, Viktor Leis et al. (TU Munich)
**Topic:** Systematic evaluation of query optimizer cardinality estimation

## Key Findings

### 1. Cardinality Estimation Errors Are Systematic
- Tested PostgreSQL, HyPer, and 3 other systems on JOB (Join Order Benchmark)
- ALL systems showed large estimation errors for multi-join queries
- Median error grows exponentially with number of joins
- For 5+ table joins, errors of 100x-10000x are common

### 2. Independence Assumption is Primary Error Source
- Assuming predicates are independent causes consistent underestimation
- Real-world data has strong correlations between columns
- Example: city='Munich' AND country='Germany' are correlated
  - Independent estimate: sel(city) * sel(country) << actual
- Multi-column statistics (v14+) help but are not created by default

### 3. Join Estimation Errors Compound
- Each join multiplies estimation error
- 2x per-join error -> 2^N total error for N joins
- This is why optimizers degrade for multi-join queries
- "Even if each individual estimate is within a factor of 2, a 10-table query can have 1000x total error"

### 4. Good Plans Don't Need Perfect Estimates
- Robust plans: similar cost across wide range of cardinalities
- Optimizer should prefer robust plans over fragile optimal plans
- Hash join more robust than nested loop (less sensitive to cardinality)
- Full table scan more robust than index scan for uncertain selectivity

### 5. Simple Heuristics Competitive with Cost-Based
- For many queries, simple heuristics (largest table first, push predicates)
  produce plans within 2x of optimal
- Cost-based optimizer adds value mainly for complex join ordering
- Suggests "paranoid" optimization: verify cost-based plan against heuristic

### 6. Join Order Matters More Than Algorithm Choice
- Bad join order: 100x-10000x slowdown
- Wrong algorithm: typically 2-5x slowdown
- Implication: invest more in join ordering quality

## Recommended Strategies

### For Optimizer Builders
1. **Use actual cardinality when available** (from prior executions)
2. **Detect estimation errors** after execution, flag for re-optimization
3. **Prefer robust plans**: avoid plans sensitive to estimate accuracy
4. **Consider worst-case estimates** for uncertain predicates
5. **Multi-column statistics**: detect correlated columns automatically
6. **Sampling-based estimation**: sample-execute subqueries for estimates

### For Cost Models
1. Don't trust cardinality estimates for plans with many joins
2. Apply estimation error safety margin (multiply estimated card by 2-3x)
3. Use hash join as default (most robust to estimation errors)
4. Avoid nested loop unless index is available (fragile)

## Applicable to Ra

### New Rule Ideas
1. **Robust Plan Preference**: When two plans have similar estimated cost
   but different sensitivity to cardinality errors, prefer the robust one.
2. **Estimation Error Safety Margin**: For joins > 3 tables, multiply
   estimated cardinality by safety factor based on join depth.
3. **Hash Join Default Policy**: Prefer hash join over nested loop unless
   inner side has confirmed index with high selectivity.
4. **Correlation Detection Advisor**: Detect when predicates on correlated
   columns produce estimation errors, recommend multi-column statistics.
5. **Post-Execution Error Logging**: Compare estimated vs actual rows per
   operator, flag operators with > 10x error.
6. **Heuristic Verification**: Run cost-based plan AND heuristic plan,
   choose the cheaper one (paranoid optimization).

### Impact
- Addresses the fundamental weakness in all cost-based optimizers
- Leis paper is the most-cited optimizer paper of the decade
- Every production optimizer has adapted based on these findings

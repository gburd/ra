# CMU 15-721 Lecture 15: Plan Stitch and Robust Query Optimization

**Source:** CMU 15-721 Spring 2024, Lecture 15 (Optimizer Implementation III)
**Date:** 2024-03-25
**Topic:** Plan stability, robust optimization, and learning from execution history
**Key Papers:** Plan Stitch (VLDB 2018), Adaptive QP in the Looking Glass (2005), Neo (VLDB 2019)

## Key Points

This lecture covers the problem of query plan regression and techniques to make
optimizers more robust. The core insight: a single "optimal" plan is fragile when
statistics are imprecise or data distributions shift.

### The Plan Regression Problem

1. **Statistics drift**: As data changes, previously optimal plans become suboptimal
2. **Parameter sensitivity**: Same query with different parameter values needs different plans
3. **Optimizer upgrades**: New optimizer versions may change plan selection unpredictably
4. **Cardinality cliffs**: Small changes in estimated cardinality cause dramatic plan switches

### Plan Stitch: Combining Best Subplans

Microsoft's Plan Stitch takes a novel approach to plan regression:

**Core algorithm:**
1. Maintain a history of all executed plans for each query template
2. Decompose each historical plan into subplans at materialization boundaries
3. For each subplan, record actual execution cost from runtime telemetry
4. When re-optimizing, stitch together the cheapest subplans from different historical plans
5. Verify stitched plan correctness (output equivalence)

**Stitching rules:**
- Two subplans are compatible if they produce the same logical output (same columns, same rows)
- Subplans can be stitched at any materialization point (hash table build, sort, temp table)
- The stitched plan cost = sum of actual subplan costs (more accurate than estimated costs)

**Key insight:** The plan space explored by Plan Stitch is a superset of any single
optimizer invocation, because it combines fragments that were individually optimal
at different points in time.

**Performance:** Up to 100x improvement over reverting to cheapest historical plan

### Adaptive Query Processing in the Looking Glass

Retrospective analysis of adaptive techniques:

1. **Eddies** (Avnur & Hellerstein 2000): Route tuples through operators adaptively
   - Per-tuple routing decision based on observed selectivities
   - No need for accurate cardinality estimation upfront
   - High overhead for simple queries, beneficial for long-running analytics

2. **Progressive Optimization** (Markl et al. 2004):
   - Insert "checkpoints" at materialization points
   - Compare estimated vs actual cardinality at each checkpoint
   - Re-optimize remainder of plan if estimation error exceeds threshold
   - More practical than Eddies for production systems

3. **Parametric Query Optimization**:
   - Pre-compute optimal plans for parameter value ranges
   - At runtime, select plan based on actual parameter values
   - Avoids re-optimization but requires upfront analysis

### Neo: Learned Query Optimization

Neural network-based optimizer that learns from query execution:

1. **Feature extraction**: Convert query plan trees to feature vectors
2. **Cost prediction**: Train neural network to predict actual execution cost
3. **Plan generation**: Use learned cost model to guide plan enumeration
4. **Continuous learning**: Update model as new queries are executed

**Key finding:** Neo can outperform hand-tuned cost models after sufficient training
data, but requires thousands of query executions to converge.

## Optimization Rules for Ra

### New Rules Identified

1. **plan-checkpoint-insertion** - Insert monitoring checkpoints at materialization points
   in the query plan (hash table build, sort completion, temp table creation)
2. **re-optimization-trigger** - When actual cardinality at checkpoint exceeds threshold
   deviation from estimate, trigger re-optimization of downstream plan
3. **parametric-plan-selection** - Maintain multiple plans indexed by parameter ranges,
   select at runtime based on actual parameter values
4. **plan-history-cache** - Cache executed plans with their actual costs for Plan Stitch
5. **subplan-stitch-candidate-generation** - Decompose cached plans into stitchable fragments
6. **robust-plan-preference** - When multiple plans have similar estimated cost, prefer
   the plan that degrades most gracefully under cardinality estimation errors

### Ra Gap Analysis

Ra currently has:
- `rules/execution-models/adaptive/` - 11 adaptive execution rules
- `crates/ra-adaptive/` - Adaptive optimization crate
- `crates/ra-regression/` - Query regression detection
- No plan history or Plan Stitch capability
- No checkpoint-based re-optimization
- No parametric plan caching

**Missing capabilities:**
- Plan fragment decomposition and stitching
- Checkpoint insertion at materialization boundaries
- Mid-execution re-optimization infrastructure
- Parameter-sensitive plan caching (plan per parameter range)
- Cost model correction from execution feedback

## Relevance to Ra

**Priority:** High - Plan regression is a primary concern for production use. Ra's
regression detection crate detects the problem but the system lacks mechanisms to
prevent or recover from plan regression.

**Proposed RFCs:**
1. **Parametric Plan Caching** - Maintain plan variants indexed by parameter value
   ranges, inspired by SQL Server's parameter sensitivity plan optimization
2. **Progressive Re-optimization** - Insert cardinality checkpoints and trigger
   re-optimization when estimates are significantly wrong

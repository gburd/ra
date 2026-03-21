# experimental Rules

Total rules in this category:       56

## Overview

Experimental rules explore cutting-edge optimization techniques from recent research including ML-guided optimization, approximate query processing, and hardware acceleration.

## Subcategories

- [adaptive](./adaptive/) -       16 rules
- [approximate](./approximate/) -        6 rules
- [compilation](./compilation/) -        3 rules
- [hardware-accel](./hardware-accel/) -        3 rules
- [ml-guided](./ml-guided/) -       11 rules
- [semantic](./semantic/) -        7 rules
- [wcoj](./wcoj/) -       10 rules

## Rules

- [Adaptive Aggregation Strategy](./adaptive-aggregation.md) - `adaptive-aggregation`
- ["Adaptive Indexing (Database Cracking)"](./adaptive-indexing.md) - `adaptive-indexing`
- [Adaptive Join Algorithm Selection](./adaptive-join-selection.md) - `adaptive-join-selection`
- [Eddies Adaptive Query Routing](./eddies-routing.md) - `eddies-routing`
- [EDDY Adaptive Query Routing](./eddy-operator.md) - `eddy-operator`
- [LEO Learning Optimizer Statistics Feedback](./leo-statistics-feedback.md) - `leo-statistics-feedback`
- [Mid-Query Re-Optimization](./mid-query-reoptimization.md) - `mid-query-reoptimization`
- [Mid-Query Re-optimization](./mid-query-replanning.md) - `mid-query-replanning`
- [Parametric Query Optimization](./parametric-query-optimization.md) - `parametric-query-optimization`
- [Plan Stability vs Adaptiveness Control](./plan-stability-control.md) - `plan-stability-control`
- [Progressive Query Optimization (Rio)](./progressive-optimization.md) - `progressive-optimization`
- [Query Result Caching and Materialized Subexpression Reuse](./query-result-caching.md) - `query-result-caching`
- [Ripple Join for Online Aggregation](./ripple-join.md) - `ripple-join`
- ["Robust Query Optimization with Worst-Case Guarantees"](./robust-query-optimization.md) - `robust-query-optimization`
- [Runtime Cardinality Feedback Loop](./runtime-cardinality-feedback.md) - `runtime-cardinality-feedback`
- [Runtime Plan Switching with Checkpoint Operators](./runtime-plan-switching.md) - `runtime-plan-switching`
- [Approximate COUNT(DISTINCT) via HyperLogLog](./approximate-count-distinct.md) - `approximate-count-distinct`
- [Approximate Percentile via t-digest/DDSketch](./approximate-percentile.md) - `approximate-percentile`
- ["Approximate Query Processing"](./approximate-query-processing.md) - `approximate-query-processing`
- [Sample-Based Approximate Aggregation](./sample-based-aggregation.md) - `sample-based-aggregation`
- [Sample-Based Join for Approximate Aggregation](./sample-based-join.md) - `sample-based-join`
- [Probabilistic Sketches for Distributed Aggregation](./sketches-for-aggregation.md) - `sketches-for-aggregation`
- ["Query Compilation vs Interpretation Cost Tradeoff"](./compilation-cost-tradeoff.md) - `compilation-cost-tradeoff`
- ["JIT Query Compilation"](./query-compilation-jit.md) - `query-compilation-jit`
- [Vectorized vs Compiled Execution Selection](./vectorized-vs-compiled.md) - `vectorized-vs-compiled`
- ["FPGA-Accelerated Query Processing"](./fpga-query-acceleration.md) - `fpga-query-acceleration`
- ["GPU Query Execution Offloading"](./gpu-offloading.md) - `gpu-offloading`
- ["Quantum-Inspired Query Optimization"](./quantum-inspired-optimization.md) - `quantum-inspired-optimization`
- [Learned Cardinality Estimation](./learned-cardinality.md) - `learned-cardinality`
- [Learned Cost Model Calibration](./learned-cost-calibration.md) - `learned-cost-calibration`
- ["Learned Cost Models"](./learned-cost-models.md) - `learned-cost-models`
- ["Learned Index Structures for Query Optimization"](./learned-index-structures.md) - `learned-index-structures`
- [Learned Join Ordering (Neo/Bao)](./learned-join-ordering.md) - `learned-join-ordering`
- [Learned Query Scheduling and Resource Allocation](./learned-query-scheduling.md) - `learned-query-scheduling`
- ["ML-Based Cardinality Estimation"](./ml-cardinality-estimation.md) - `ml-cardinality-estimation`
- [ML-Based Plan Hint Generation](./plan-hint-generation.md) - `plan-hint-generation`
- ["Query Embedding Similarity for Plan Reuse"](./query-embedding-similarity.md) - `query-embedding-similarity`
- ["Reinforcement Learning for Join Ordering"](./rl-join-ordering.md) - `rl-join-ordering`
- [ML-Guided Workload-Aware Index Selection](./workload-aware-indexing.md) - `workload-aware-indexing`
- [Commutativity-Aware Algebraic Rewriting](./commutativity-aware-rewriting.md) - `commutativity-aware-rewriting`
- [Constraint-Based Semantic Rewriting](./constraint-based-rewriting.md) - `constraint-based-rewriting`
- [E-graph Extraction Strategies for Query Optimization](./egg-extraction-strategies.md) - `egg-extraction-strategies`
- [Equality Saturation Query Rewriting](./equality-saturation.md) - `equality-saturation`
- [Functional Dependency-Based Rewriting](./functional-dependency-rewrite.md) - `functional-dependency-rewrite`
- [HoTTSQL Proof-Based Query Rewriting](./hottsql-proof-rewrite.md) - `hottsql-proof-rewrite`
- [Semi-Join Reduction Programs](./semijoin-reduction.md) - `semijoin-reduction`
- [Delta WCOJ for Incremental Maintenance](./delta-wcoj.md) - `delta-wcoj`
- [Factorized Join Representation](./factorized-join.md) - `factorized-join`
- [Free Join (Worst-Case Optimal Join)](./free-join.md) - `free-join`
- [Generic Join (Ngo-Porat-Re-Rudra)](./generic-join.md) - `generic-join`
- [HoneyComb Distributed WCOJ](./honeycomb-join.md) - `honeycomb-join`
- [LeapFrog TrieJoin](./leapfrog-triejoin.md) - `leapfrog-triejoin`
- [LevelHeaded Join Algorithm](./level-headed-join.md) - `level-headed-join`
- [WCOJ for Clique Detection Patterns](./wcoj-clique-detection.md) - `wcoj-clique-detection`
- [WCOJ for Star Join Patterns](./wcoj-star-pattern.md) - `wcoj-star-pattern`
- [WCOJ to Binary Join Fallback](./wcoj-to-binary-fallback.md) - `wcoj-to-binary-fallback`

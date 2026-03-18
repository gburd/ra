# Modern Research Rules (2015-2025)

Complete index of 30 modern research optimization rules extracted from recent academic papers and cutting-edge database research.

## WCOJ Algorithms (8 rules)

1. **free-join.rra** ✅ - Free Join worst-case optimal join (Ngo et al. 2012)
2. **leapfrog-triejoin.rra** ✅ - LeapFrog TrieJoin (Veldhuizen 2014)
3. **honeycomb-join.rra** - HoneyComb parallel WCOJ (Facebook 2021)
4. **tetris-join.rra** - Generic Join for heterogeneous data
5. **factorized-join.rra** - Factorized query evaluation (Olteanu & Schleich 2016)
6. **yannakakis-algorithm.rra** - Acyclic query optimization (Yannakakis 1981, modern impl)
7. **delta-leapfrog.rra** - Incremental WCOJ for streaming (EmptyHeaded)
8. **minesweeper-join.rra** - Light-weight cardinality bounding (Freitag et al. 2020)

## Semantic Rewriting (7 rules)

9. **hottSQL-type-rewrite.rra** - HoTTSQL type-directed query rewriting (Chu et al. 2017)
10. **cosette-equiv-check.rra** - Cosette equivalence verification
11. **equality-saturation.rra** - E-graph equality saturation (egg library)
12. **query-by-example-synthesis.rra** - SQL synthesis from I/O examples
13. **schema-mapping-rewrite.rra** - Schema evolution query rewriting
14. **semantic-optimization.rra** - Integrity constraint exploitation
15. **view-based-rewrite.rra** - Automatic view materialization selection

## Adaptive Execution (8 rules)

16. **eddy-operator.rra** - EDDY adaptive routing (Avnur & Hellerstein 2000, modern)
17. **mid-query-replan.rra** - Runtime plan switching
18. **progressive-execution.rra** - Progressive query processing
19. **morsel-driven-parallelism.rra** - Work-stealing execution (HyPer)
20. **adaptive-code-generation.rra** - Just-in-time plan compilation
21. **runtime-filter-pushdown.rra** - Bloom filter injection during execution
22. **statistics-feedback.rra** - Online statistics refinement
23. **adaptive-batching.rra** - Dynamic batch size tuning

## ML-Guided Optimization (7 rules)

24. **learned-cardinality.rra** - Neural cardinality estimation (MSCN, NeuroCard)
25. **learned-join-order.rra** - Deep RL join ordering (DQ, Bao)
26. **learned-cost-model.rra** - ML cost model (Neo, TPCH learned)
27. **query-plan-selection.rra** - Plan selection via multi-armed bandits
28. **cardinality-correction.rra** - Runtime cardinality feedback and correction
29. **workload-driven-tuning.rra** - Workload-aware index/view selection
30. **learned-predicate-selectivity.rra** - ML selectivity estimation

## Implementation Status

- ✅ Completed: 2/30
- 🚧 In Progress: 28/30
- Total LOC target: ~6000 lines (avg 200 lines per rule)

## References

### Key Papers

**WCOJ:**
- Ngo et al., "Worst-Case Optimal Join Algorithms", PODS 2012
- Veldhuizen, "Triejoin", ICDT 2014
- Freitag et al., "WCOJoin on GPUs", SIGMOD 2020
- Aberger et al., "EmptyHeaded", SIGMOD 2016

**Semantic:**
- Chu et al., "HoTTSQL", PLDI 2017
- Wang et al., "Cosette", CIDR 2018
- Willsey et al., "egg: Fast and Extensible Equality Saturation", POPL 2021

**Adaptive:**
- Avnur & Hellerstein, "Eddies", SIGMOD 2000
- Neumann, "Morsel-Driven Parallelism", SIGMOD 2014
- Dursun et al., "Umbra Adaptive Execution", CIDR 2020

**ML-Guided:**
- Kipf et al., "Learned Cardinalities", CIDR 2019
- Marcus et al., "Neo", SIGMOD 2019
- Marcus et al., "Bao", SIGMOD 2021
- Hilprecht et al., "DeepDB", SIGMOD 2020
- Yang et al., "NeuroCard", VLDB 2021

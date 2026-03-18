# Modern Research Rules (2015-2025)

Complete index of 30 modern research optimization rules extracted from recent academic papers and cutting-edge database research.

## WCOJ Algorithms (10 rules)

| File | Status | Description | Source |
|------|--------|-------------|--------|
| free-join.rra | Done | Free Join worst-case optimal join | Ngo et al. 2012 |
| leapfrog-triejoin.rra | Done | LeapFrog TrieJoin | Veldhuizen 2014 |
| generic-join.rra | Done | Generic Join (Ngo-Porat-Re-Rudra) | Ngo et al. 2014/2018 |
| level-headed-join.rra | Done | LevelHeaded with aggregation pushdown | Aberger et al. 2018 |
| honeycomb-join.rra | Done | HoneyComb distributed WCOJ (Shares) | Chu et al. 2015 |
| wcoj-star-pattern.rra | Done | WCOJ for star join with correlated filters | Ngo et al. 2014 |
| wcoj-clique-detection.rra | Done | WCOJ for k-clique subgraph patterns | Aberger et al. 2017 |
| factorized-join.rra | Done | Factorized join representation | Olteanu & Zavodny 2015 |
| wcoj-to-binary-fallback.rra | Done | Hybrid WCOJ/binary with runtime switching | Freitag et al. 2020 |
| delta-wcoj.rra | Done | Delta WCOJ for incremental maintenance | Kim et al. 2022 |

## Semantic Rewriting (7 rules)

| File | Status | Description | Source |
|------|--------|-------------|--------|
| equality-saturation.rra | Done | E-graph equality saturation | Willsey et al. 2021 |
| hottsql-proof-rewrite.rra | Done | HoTTSQL proof-based rewrites | Chu et al. 2017 |
| commutativity-aware-rewriting.rra | Done | Structural commutativity/associativity | Tate et al. 2009 |
| constraint-based-rewriting.rra | Done | Integrity constraint exploitation (Chase) | Chandra & Merlin 1977 |
| egg-extraction-strategies.rra | Done | Multi-objective E-graph extraction | Willsey et al. 2021 |
| functional-dependency-rewrite.rra | Done | FD-based GROUP BY/ORDER BY reduction | Simmen et al. 1996 |
| semijoin-reduction.rra | Done | Semi-join reduction programs (bloom filters) | Bernstein & Chiu 1981 |

## Adaptive Execution (7 rules)

| File | Status | Description | Source |
|------|--------|-------------|--------|
| eddy-operator.rra | Done | EDDY adaptive routing | Avnur & Hellerstein 2000 |
| adaptive-join-selection.rra | Done | Runtime join algorithm switching | Babu et al. 2005 |
| mid-query-replanning.rra | Done | Mid-query re-optimization with checkpoints | Kabra & DeWitt 1998 |
| runtime-cardinality-feedback.rra | Done | Cardinality feedback loop (LEO) | Stillger et al. 2001 |
| progressive-optimization.rra | Done | Plan envelopes with validity ranges | Markl et al. 2004 |
| adaptive-aggregation.rra | Done | Hash-to-sort aggregation switching | Leis et al. 2015 |
| ripple-join.rra | Done | Ripple join for online aggregation | Haas & Hellerstein 1999 |

## ML-Guided Optimization (6 rules)

| File | Status | Description | Source |
|------|--------|-------------|--------|
| learned-cardinality.rra | Done | Neural cardinality estimation (MSCN/NeuroCard) | Kipf et al. 2019 |
| learned-join-ordering.rra | Done | Deep RL join ordering (Neo/Bao) | Marcus et al. 2019/2021 |
| learned-cost-calibration.rra | Done | Hardware-specific cost model calibration | Sun et al. 2019 |
| plan-hint-generation.rra | Done | ML-based optimizer hint generation | Marcus et al. 2021 |
| workload-aware-indexing.rra | Done | ML-guided index recommendation | Ding et al. 2019 |
| learned-query-scheduling.rra | Done | Learned query scheduling and resource allocation | Chi et al. 2021 |

## Implementation Status

- Done: 30/30
- Total rules: 30 files across 4 categories
- New operator types documented: free_join, generic_join, leapfrog_triejoin,
  levelheaded_join, honeycomb_join, wcoj_clique, factorized_join, delta_wcoj,
  adaptive_join, plan_envelope, ripple_join

## Key Operator Types Introduced

### Requires new join operators
- `free_join` / `generic_join` / `leapfrog_triejoin` -- multi-way WCOJ joins
- `levelheaded_join` -- WCOJ with integrated aggregation (semiring annotations)
- `honeycomb_join` -- distributed WCOJ with Shares partitioning
- `wcoj_clique` -- specialized clique detection join
- `factorized_join` -- join producing factorized (compressed) output
- `delta_wcoj` -- incremental WCOJ for view maintenance

### Requires runtime adaptation hooks
- `adaptive_join` -- runtime algorithm switching (hash/merge/NL)
- `plan_envelope` -- pre-compiled plan alternatives with switch points
- `checkpoint` -- cardinality checkpoint for mid-query replanning
- `ripple_join` -- interleaved sampling join for online aggregation
- `eddy` -- tuple-at-a-time adaptive routing

### Requires external ML models
- `learned_cardinality` -- neural cardinality estimation (requires trained MSCN/NeuroCard)
- `neo_join_order` / `bao_select` -- learned join ordering (requires execution history)
- `learned_cost` -- calibrated cost model (requires hardware measurements)
- `hint_generator` -- ML hint prediction (requires workload feedback)
- `index_advisor` -- ML index recommendation (requires workload tracking)
- `query_scheduler` -- learned resource allocation (requires execution metrics)

## References

### Key Papers

**WCOJ:**
- Ngo et al., "Worst-Case Optimal Join Algorithms", PODS 2012 / JACM 2018
- Veldhuizen, "Triejoin", ICDT 2014
- Aberger et al., "EmptyHeaded", SIGMOD 2017
- Aberger et al., "LevelHeaded", SIGMOD 2018
- Chu et al., "From Theory to Practice: Efficient Join Query Evaluation", SIGMOD 2015
- Olteanu, Zavodny, "Size Bounds for Factorised Representations", TODS 2015
- Freitag et al., "Adopting WCOJ in Relational Database Systems", VLDB 2020
- Kim et al., "Incremental View Maintenance with Triple Lock Factorization", SIGMOD 2022

**Semantic:**
- Chu et al., "HoTTSQL", PLDI 2017
- Willsey et al., "egg: Fast and Extensible Equality Saturation", POPL 2021
- Tate et al., "Equality Saturation: A New Approach to Optimization", POPL 2009
- Simmen et al., "Fundamental Techniques for Order Optimization", SIGMOD 1996
- Bernstein, Chiu, "Using Semi-Joins to Solve Relational Queries", JACM 1981
- Chandra, Merlin, "Optimal Implementation of Conjunctive Queries", STOC 1977

**Adaptive:**
- Avnur, Hellerstein, "Eddies", SIGMOD 2000
- Kabra, DeWitt, "Efficient Mid-Query Re-Optimization", SIGMOD 1998
- Markl et al., "Robust Query Processing through Progressive Optimization", SIGMOD 2004
- Stillger et al., "LEO - DB2's Learning Optimizer", VLDB 2001
- Haas, Hellerstein, "Ripple Joins for Online Aggregation", SIGMOD 1999

**ML-Guided:**
- Kipf et al., "Learned Cardinalities", CIDR 2019
- Marcus et al., "Towards a Hands-Free Query Optimizer (Neo)", CIDR 2019
- Marcus et al., "Bao: Making Learned Query Optimization Practical", SIGMOD 2021
- Sun et al., "An End-to-End Learning-based Cost Estimator", VLDB 2019
- Ding et al., "AI Meets AI: Leveraging Query Executions for Index Recommendations", SIGMOD 2019
- Chi et al., "Learned Scheduling for Data Processing Clusters", MLSys 2021

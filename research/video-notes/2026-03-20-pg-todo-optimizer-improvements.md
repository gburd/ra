# PostgreSQL TODO: Optimizer Improvements

**Source:** https://wiki.postgresql.org/wiki/Todo
**Date:** Reference (current)
**Speaker:** PostgreSQL community

## Key Points
- PostgreSQL maintains a public TODO list of desired optimizer improvements
- Several improvements have been pending for years
- These represent known gaps in PostgreSQL's optimizer

## Planned/Desired Improvements

### Selectivity Estimation
1. **Geometric operator selectivity**: improve estimates for spatial queries
2. **Cardinality-reducing functions**: recognize that int4mod, date_trunc, date_bin reduce cardinality - current assumption is functions don't meaningfully change cardinality
3. **Estimation error logging**: log when actual rows dramatically differ from estimates
4. **Rejected path visibility**: expose paths considered but rejected (beyond OPTIMIZER_DEBUG)

### Join Optimization
1. **Hash join pathkeys**: preserve outer ordering through single-batch hash joins
2. **Avoid duplicate hash tables**: reuse hash tables when same relation appears multiple times in query
3. **DISTINCT-to-join hash reuse**: avoid re-hashing when DISTINCT precedes a hash join on same columns
4. **Cartesian join consideration**: consider Cartesian products when both relations needed for index scan on third

### Search Strategy
1. **GEQO replacement**: investigate compressed annealing as alternative to genetic algorithm
2. **Better heuristics**: improve join ordering heuristics for GEQO fallback

### Index Optimization
1. **Extensible special index operators**: make index operator mechanism pluggable for new index types

## Applicable to RA
- Gap: No function cardinality estimation (date_trunc, int4mod awareness)
- Gap: No hash table reuse detection across operators
- Gap: No Cartesian join consideration for index enablement
- Gap: No compressed annealing search strategy
- Gap: No estimation error logging/detection mechanism
- Gap: No pathkey preservation through hash joins

## References
- PostgreSQL wiki: https://wiki.postgresql.org/wiki/Todo
- PostgreSQL mailing lists: pgsql-hackers

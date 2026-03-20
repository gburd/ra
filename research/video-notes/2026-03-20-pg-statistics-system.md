# PostgreSQL Statistics System Deep Dive

**Source:** https://www.postgresql.org/docs/current/planner-stats.html
**Date:** Reference documentation (current)
**Speaker:** PostgreSQL documentation

## Key Points
- Statistics are the foundation of cost-based optimization
- PostgreSQL stores statistics in pg_statistic, accessed via pg_stats view
- Extended (multivariate) statistics address correlated columns
- ANALYZE command gathers statistics via random sampling

## Single-Column Statistics

### Components
1. **null_frac**: fraction of NULL values
2. **n_distinct**: number of distinct values (negative = fraction of rows)
3. **most_common_vals**: array of most common values
4. **most_common_freqs**: frequencies of most common values
5. **histogram_bounds**: equi-depth histogram bucket boundaries
6. **correlation**: physical vs logical ordering correlation (-1 to 1)

### Configuration
- default_statistics_target: 100 (number of histogram buckets / MCV entries)
- Per-column: ALTER TABLE SET STATISTICS
- Higher values = more accurate but more storage and ANALYZE time

### Selectivity Estimation Formulas
- Equality (in MCV): selectivity = frequency from most_common_freqs
- Equality (not in MCV): (1 - sum(mcv_freqs)) / (n_distinct - num_mcv)
- Range: linear interpolation within histogram buckets
- Combined (MCV + histogram): mcv_sel + histogram_sel * non_mcv_fraction
- AND: multiply individual selectivities (independence assumption)
- Join: (1-null_frac1) * (1-null_frac2) / max(n_distinct1, n_distinct2)

## Extended Statistics (Multivariate)

### Functional Dependencies
- CREATE STATISTICS stts (dependencies) ON city, zip FROM zipcodes
- Coefficient range 0.0-1.0 indicating dependency strength
- Fixes independence assumption for correlated columns
- Limitation: only equality conditions and IN clauses

### Multivariate N-Distinct
- CREATE STATISTICS stts (ndistinct) ON city, state, zip FROM zipcodes
- Improves GROUP BY cardinality estimation
- Stores distinct counts for all column subsets

### Multivariate MCV Lists
- CREATE STATISTICS stts (mcv) ON city, state FROM zipcodes
- Stores common value combinations with actual frequencies
- Compares actual vs independence-assumed frequencies
- Example: Washington DC appears 100x more than independence predicts

## Applicable to RA
- RA has ra-stats crate and cost-models/ with estimation rules
- Gap: No automatic extended statistics recommendation
- Gap: No staleness detection (how old are statistics?)
- Gap: No per-column statistics target tuning advisor
- Gap: No functional dependency detection for automatic statistics creation
- Gap: No histogram type selection (equi-width vs equi-depth vs V-optimal)
- Gap: No statistics propagation through operator trees
- Gap: No cross-column correlation detection without explicit CREATE STATISTICS

## References
- PostgreSQL source: src/backend/utils/adt/selfuncs.c
- PostgreSQL source: src/backend/optimizer/path/clausesel.c

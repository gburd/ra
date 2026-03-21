# PostgreSQL: Cardinality Estimation Formulas

**Source:** PostgreSQL Documentation (Row Estimation Examples)
**Topic:** Detailed cardinality estimation formulas used by PostgreSQL

## Key Formulas

### 1. Base Table Cardinality
```
rows = reltuples * (current_pages / relpages)
```
Scales stored row count by actual-to-stored page ratio to account for
changes since last ANALYZE.

### 2. Equality Predicate (col = value)

**If value in MCV list:**
```
selectivity = most_common_freqs[value_index]
```

**If value NOT in MCV list:**
```
selectivity = (1 - sum(mcv_freqs)) / (n_distinct - num_mcv)
```
Assumes remaining values uniformly distributed among non-MCV values.

### 3. Range Predicate (col < value)

**Using histogram:**
```
full_buckets = count of buckets entirely below value
partial = (value - bucket_lower) / (bucket_upper - bucket_lower)
selectivity = (full_buckets + partial) / num_buckets
```

**Combined MCV + histogram:**
```
mcv_sel = sum of frequencies where MCV value matches predicate
hist_sel = histogram selectivity for non-MCV population
selectivity = mcv_sel + hist_sel * (1 - sum(mcv_freqs))
```

### 4. AND Combination
```
selectivity = sel(cond1) * sel(cond2)
```
Independence assumption. Known to underestimate when columns are correlated.

### 5. OR Combination
```
selectivity = 1 - (1 - sel(cond1)) * (1 - sel(cond2))
```
Inclusion-exclusion with independence assumption.

### 6. NOT
```
selectivity = 1 - sel(cond)
```

### 7. Equijoin (A.col = B.col)

**When both columns unique:**
```
selectivity = 1 / max(n_distinct_A, n_distinct_B)
join_rows = rows_A * rows_B * selectivity
```

**When MCV lists available:**
```
mcv_sel = sum(freq_A[i] * freq_B[i]) for matching MCV values
non_mcv_sel = (1 - sum_freq_A) * (1 - sum_freq_B) / max(ndist_A - mcv_A, ndist_B - mcv_B)
selectivity = mcv_sel + non_mcv_sel
```

### 8. Semi-Join (EXISTS)
```
selectivity = 1 - (1 - 1/n_distinct_inner)^n_outer
```
Probability that at least one inner row matches.

### 9. Anti-Join (NOT EXISTS)
```
selectivity = (1 - 1/n_distinct_inner)^n_inner_per_key
```
Probability that no inner row matches for a given outer key.

### 10. GROUP BY Cardinality
```
result_rows = product(n_distinct(col)) for each GROUP BY column
```
Capped at input row count. Independence assumption again.

### 11. DISTINCT Cardinality
```
result_rows = n_distinct of the column set
```
Estimated using formula from Haas & Stokes (1998) for multi-column distinct.

## Extended Statistics (v14+)

### Functional Dependencies
```
When A -> B (functional dependency detected):
  selectivity(A = a AND B = b) = sel(A = a) * (1 - dep + dep * sel(B = b | A = a))
```
Where `dep` is dependency degree (0 = independent, 1 = fully dependent).

### Multivariate NDV
```
n_distinct(A, B) = measured value from CREATE STATISTICS
-- Instead of heuristic: min(ndv_A * ndv_B, total_rows)
```

### Multivariate MCV
```
For correlated columns, maintains MCV list on column combinations.
Selectivity for combined predicate read directly from joint MCV.
```

## Applicable to Ra

### New Rule Ideas
1. **Extended Statistics Advisor**: When estimation error detected for
   correlated columns, recommend CREATE STATISTICS.
2. **Multi-Column NDV Estimation**: Use joint distinct count instead of
   product of individual NDVs for GROUP BY estimation.
3. **Semi-Join Cardinality Formula**: Use probability formula instead of
   heuristic for EXISTS subquery estimation.
4. **Anti-Join Cardinality Formula**: Use complement probability for
   NOT EXISTS estimation.
5. **Histogram-Based Range Selectivity**: Use histogram bucket boundaries
   for accurate range predicate estimation.
6. **MCV-Aware Join Selectivity**: When MCV lists available for both join
   columns, use direct frequency comparison instead of independence.

### Gap Analysis
- Ra has basic selectivity (1/NDV for equality)
- Ra has histograms (equi-width and equi-depth)
- Missing: MCV-aware selectivity
- Missing: combined MCV + histogram estimation
- Missing: extended statistics support
- Missing: semi-join / anti-join cardinality formulas
- Missing: functional dependency detection

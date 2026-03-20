# PostgreSQL Cost Estimation Formulas

**Source:** https://www.postgresql.org/docs/current/row-estimation-examples.html + The Internals of PostgreSQL
**Date:** Reference documentation (current)
**Speaker:** PostgreSQL documentation / Hironobu Suzuki

## Key Points
- Cost is measured in arbitrary relative units (seq_page_cost = 1.0)
- Three cost components: startup, run, total (= startup + run)
- Row estimation uses histograms, MCVs, and sampling
- Join estimation assumes independence and uniform distribution

## Cost Formulas

### Sequential Scan
- startup_cost = 0
- run_cost = (cpu_tuple_cost + cpu_operator_cost) * N_tuples + seq_page_cost * N_pages
- Example: (0.01 + 0.0025) * 10000 + 1.0 * 345 = 470

### Index Scan
- startup_cost = ceil(log2(N_index)) * (H + 1) * 50 * cpu_operator_cost
- run_cost = index_cpu + table_cpu + index_io + table_io
- index_cpu = selectivity * N_index * (cpu_index_tuple_cost + qual_op_cost)
- table_cpu = selectivity * N_tuples * cpu_tuple_cost
- index_io = ceil(selectivity * N_index_pages) * random_page_cost
- table_io = f(correlation, selectivity, N_pages) * page_cost
  - correlation near 1.0: mostly sequential reads
  - correlation near 0.0: mostly random reads

### Sort
- startup_cost = comparison_cost * N * log2(N)
- comparison_cost = 2 * cpu_operator_cost
- run_cost = cpu_operator_cost * N

### Nested Loop Join
- startup_cost = outer_startup
- run_cost = outer_run + N_outer * (inner_total + inner_cpu_per_tuple)

### Merge Join
- startup_cost = outer_sort_startup + inner_sort_startup
- run_cost = outer_sort_run + inner_sort_run + merge_cost
- merge_cost = (N_outer + N_inner) * cpu_operator_cost * cpu_tuple_cost

### Hash Join
- startup_cost = inner_total + hash_build_cost
- hash_build_cost = inner_tuples * cpu_operator_cost
- run_cost = outer_total + hash_probe_cost + output_cost
- hash_probe_cost = outer_tuples * cpu_operator_cost

## Row Estimation Examples

### Range predicate (unique1 < 1000)
- Locate bucket in histogram_bounds containing 1000
- Linear interpolation within bucket
- selectivity = (1 + (1000 - 993)/(1997 - 993)) / 10 = 0.100697
- rows = 10000 * 0.100697 = 1007

### Equality predicate (stringu1 = 'CRAAAA')
- If in MCV list: selectivity = corresponding frequency
- If not in MCV: (1 - sum(mcv_freqs)) / (n_distinct - num_mcv)

### Join estimation (t1.unique2 = t2.unique2)
- selectivity = (1 - null_frac1) * (1 - null_frac2) / max(rows1, rows2)
- join_rows = outer_rows * inner_rows * selectivity

## Applicable to RA
- RA has cost model rules but lacks specific formula implementations
- Gap: No correlation-aware index scan cost (physical-logical order)
- Gap: No startup vs total cost distinction in cost model
- Gap: No hash build vs probe cost separation
- Gap: No sort cost model accounting for available memory
- Gap: No bitmap scan cost model (combining multiple indexes)

## References
- PostgreSQL source: src/backend/optimizer/path/costsize.c
- Suzuki. "The Internals of PostgreSQL" Chapter 3

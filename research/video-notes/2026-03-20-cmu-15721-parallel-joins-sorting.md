# CMU 15-721 Lecture 12: Parallel Join Algorithms (Sorting)

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Sort-merge joins have resurgence in modern hardware
- SIMD-friendly sorting algorithms
- Sorting preserves ordering for downstream operations
- Multi-way merge parallelization

## Parallel Sort-Merge Techniques

### Parallel External Merge Sort
- Partition input across threads for local sorting
- Multi-way merge of sorted runs
- Can use tournament trees or loser trees for efficient merging
- Network-friendly for distributed systems

### SIMD-Optimized Sorting
- Sorting networks: fixed comparison sequences, SIMD-parallel
- Bitonic sort: parallel comparison network
- Radix sort: SIMD scatter/gather operations
- Modern CPUs: 4-16x speedup with AVX-512

### Sort-Merge Join Advantages
- Output is sorted (useful for subsequent operations)
- Better worst-case behavior than hash join (no skew issues)
- Can merge in pipeline fashion
- Natural for merge of pre-sorted inputs

### When Sort-Merge Beats Hash Join
- Data already sorted (from index scan or prior sort)
- Output ordering required (ORDER BY, GROUP BY)
- Extreme skew in join keys
- Memory pressure (external merge sort is I/O efficient)

## Applicable to RA
- RA has physical/sort/ directory
- Gap: No SIMD-aware sort algorithm selection rules
- Gap: No "sort already available" detection rules
- Gap: No sort-merge vs hash join decision rules incorporating downstream ordering needs
- Gap: No external sort cost modeling with I/O patterns

## References
- Chhugani et al. "Efficient Implementation of Sorting on Multi-Core SIMD CPU Architecture" (2008)
- Albutiu, Kemper, Neumann. "Massively Parallel Sort-Merge Joins in Main Memory Multi-Core Database Systems" (2012)

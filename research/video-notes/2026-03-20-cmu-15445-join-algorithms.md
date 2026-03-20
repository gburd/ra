# CMU 15-445 Lecture 11: Join Algorithms

**Source:** https://15445.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Join algorithms are a core performance-critical component
- Choice of algorithm depends on data size, sort order, available memory, and indexes
- No single algorithm wins in all cases - cost model must select

## Join Algorithm Techniques

### Nested Loop Join
- Simple nested loop: O(M * N) - scan inner for each outer tuple
- Block nested loop: use buffer pages to reduce I/O
- Index nested loop: use index on inner relation for lookups
- Best for: small outer relation, index available on inner

### Sort-Merge Join
- Sort both relations on join key, then merge
- Cost: sort cost + merge cost (linear scan)
- Best for: already-sorted data, interesting orderings
- Can reuse sorted output for subsequent operations

### Hash Join
- Build hash table on smaller relation, probe with larger
- Simple hash join: requires inner fits in memory
- Grace hash join: partition both into buckets, join matching buckets
- Hybrid hash join: keep first partition in memory during build
- Best for: equi-joins on unsorted data, large datasets

### Multi-Way Joins (WCOJ - Worst-Case Optimal Joins)
- Leapfrog Triejoin: join N relations simultaneously
- Better asymptotic complexity for cyclic queries
- Triangle queries: O(N^{3/2}) vs O(N^2) for pairwise joins

## Cost Model Considerations
- I/O cost dominates for disk-based systems
- Memory availability determines which algorithm variants apply
- Sort-merge preserves interesting orderings for later operations
- Hash join handles skew poorly without explicit skew handling
- Bloom filters can pre-filter hash join inputs

## Applicable to RA
- RA has physical/join-algorithms/ (18 rules) covering basics
- Gap: No bloom filter pre-filtering for hash joins
- Gap: No skew detection and handling for hash joins
- Gap: No runtime algorithm switching (adaptive joins)
- Gap: Limited WCOJ/leapfrog triejoin support (only 10 experimental wcoj rules)
- Gap: No "interesting orderings" framework connecting sort-merge output to downstream ops

## References
- Shapiro. "Join Processing in Database Systems with Large Main Memories" (1986)
- Graefe. "Sort-Merge-Join: An Idea Whose Time Has(h) Passed?" (1994)
- Ngo, Porat, Re, Rudra. "Worst-Case Optimal Join Algorithms" (2012)

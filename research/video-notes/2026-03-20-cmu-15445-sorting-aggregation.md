# CMU 15-445 Lecture 10: Sorting & Aggregation Algorithms

**Source:** https://15445.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Sorting is fundamental: ORDER BY, GROUP BY, DISTINCT, merge join
- External merge sort for data exceeding memory
- Aggregation via sorting or hashing

## Sorting Techniques

### External Merge Sort
- Phase 1: Create sorted runs that fit in memory
- Phase 2: Merge sorted runs using multi-way merge
- B+1-way merge with B buffer pages
- Number of passes: 1 + ceil(log_{B}(N/B))
- Optimization: double buffering (prefetch next pages during merge)

### Top-N Heap Sort
- For ORDER BY ... LIMIT N queries
- Maintain heap of N items, scan input once
- O(n log N) instead of O(n log n) for full sort

### Interesting Orderings (System R concept)
- Some orderings are "interesting" because they benefit later operations
- Example: sort for merge join can also satisfy ORDER BY
- Planner should track and propagate ordering properties
- Avoid redundant sorts by reusing intermediate orderings

## Aggregation Techniques

### Sort-Based Aggregation
- Sort by GROUP BY columns, then linear scan to compute aggregates
- Reuses sort infrastructure
- Good when output needs to be sorted anyway

### Hash-Based Aggregation
- Hash on GROUP BY columns into partitions
- Compute aggregates per partition
- External hash aggregation for memory overflow
- Generally faster than sort-based for unordered output

## Applicable to RA
- RA has physical/aggregation-strategies/ (16 rules) and physical/sort/ directory
- Gap: No explicit "interesting orderings" propagation framework
- Gap: No Top-N optimization (ORDER BY + LIMIT fusion)
- Gap: No external sort/hash overflow cost modeling
- Gap: No double-buffering or prefetch-aware cost modeling

## References
- Knuth. "The Art of Computer Programming, Vol. 3: Sorting and Searching"
- Selinger et al. "Access Path Selection" (1979) - introduced interesting orderings

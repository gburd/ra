# CMU 15-721 Lecture 13: Multi-Way Joins

**Source:** https://15721.courses.cs.cmu.edu/spring2023/schedule.html
**Date:** 2023 (Spring semester)
**Speaker:** Andy Pavlo

## Key Points
- Traditional pairwise joins can be asymptotically suboptimal for cyclic queries
- Worst-case optimal join (WCOJ) algorithms process all relations simultaneously
- Leapfrog Triejoin is the practical implementation
- Applicable to graph pattern matching and cyclic queries

## Multi-Way Join Techniques

### Worst-Case Optimal Joins (WCOJ)
- AGM bound: upper bound on output size for any join query
- Traditional binary join plans can exceed AGM bound
- WCOJ algorithms guarantee output proportional to AGM bound
- Key insight: intersect sorted lists simultaneously

### Leapfrog Triejoin
- Maintain iterators on tries (sorted indexes) for each relation
- At each level, intersect values across all relations
- "Leapfrog" to next matching value across iterators
- Complexity: O(N^{p/2}) for cycles of size p
- Triangle query: O(N^{3/2}) vs O(N^2) for pairwise

### When WCOJ Helps
- Cyclic queries (triangles, cliques in graphs)
- Self-joins with complex patterns
- Graph pattern matching (social networks, knowledge graphs)
- NOT helpful for acyclic queries (tree-shaped joins)

### Hybrid Approaches
- Use traditional binary joins for acyclic subplans
- Switch to WCOJ for cyclic components
- Detect cycles in join graph automatically
- Some systems (EmptyHeaded, LogicBlox) use WCOJ extensively

## Applicable to RA
- RA has experimental/wcoj/ (10 rules) for worst-case optimal joins
- Gap: No automatic cycle detection in join graphs
- Gap: No hybrid binary/WCOJ plan generation
- Gap: No leapfrog triejoin cost model
- Gap: No trie index construction rules for WCOJ
- Gap: WCOJ rules are experimental - need promotion to production rules

## References
- Ngo, Porat, Re, Rudra. "Worst-Case Optimal Join Algorithms" (2012)
- Veldhuizen. "Leapfrog Triejoin: A Simple, Worst-Case Optimal Join Algorithm" (2014)
- Aberger et al. "EmptyHeaded: A Relational Engine for Graph Processing" (2016)

# CMU 15-721 Lecture 13-15: Optimizer Implementation (Cascades Framework)

**Source:** CMU 15-721 Spring 2024, Lectures 13-15
**Speaker:** Andy Pavlo
**Topic:** Optimizer Implementation I-III

## Key Concepts

### Cascades Framework
- Top-down search with memoization
- Groups represent equivalence classes of expressions
- Two types of rules: transformation (logical->logical) and implementation (logical->physical)
- Physical properties (ordering, partitioning) propagated through tree
- Branch-and-bound pruning eliminates expensive partial plans early

### Optimizer Search Strategies

**Bottom-Up (System R / Starburst)**
- Dynamic programming over join orderings
- Build optimal plans from 1-relation sets to N-relation sets
- Track "interesting orderings" at each level
- Guarantee: finds optimal plan within search space
- Weakness: exponential memory for large join counts

**Top-Down (Cascades / Volcano)**
- Start from root, recursively optimize children
- Memoize results in hash table of equivalence groups
- Physical property requirements flow top-down
- Enforcers (Sort, Exchange) inserted when properties missing
- More natural for distributed query planning

**Equality Saturation (egg)**
- Add all equivalent forms to e-graph simultaneously
- Extract optimal plan from saturated e-graph
- Advantage: no rule ordering sensitivity
- Disadvantage: can be slow for large plans, no pruning during saturation
- Ra uses this approach

### Key Differences for Ra

Ra's egg-based approach lacks:
1. Physical property tracking through plan nodes
2. Branch-and-bound pruning during search
3. Enforcer rules for Sort/Exchange insertion
4. Property requirement propagation (top-down)
5. Multi-phase optimization (heuristic cleanup then cost-based)

## Transformation Rules Discussed

### Logical-to-Logical
- Join commutativity: `A JOIN B -> B JOIN A`
- Join associativity: `(A JOIN B) JOIN C -> A JOIN (B JOIN C)`
- Selection pushdown: `sigma(A JOIN B) -> sigma(A) JOIN B` (when pred references only A)
- Projection pushdown: `pi(A JOIN B) -> pi(pi(A) JOIN pi(B))`
- Aggregate pushdown: `gamma(A JOIN B) -> gamma(A) JOIN B` (for distributive aggregates)
- Decorrelation: correlated subquery -> semi/anti join
- Outer-to-inner: LEFT JOIN with null-rejecting WHERE -> INNER JOIN

### Logical-to-Physical (Implementation)
- Scan -> SeqScan | IndexScan | BitmapScan
- Join -> NestedLoop | HashJoin | MergeJoin | IndexNLJ
- Aggregate -> HashAggregate | SortAggregate | StreamAggregate
- Sort -> FullSort | IncrementalSort | TopNSort

### Enforcer Rules
- Missing ordering -> Insert Sort node
- Missing partitioning -> Insert Exchange/Redistribute node
- Missing distribution -> Insert Broadcast/Gather node

## Cost Model Integration

- Each physical operator has cost function(s)
- Cost depends on input cardinality, available statistics, hardware profile
- Startup cost vs total cost distinction (PostgreSQL-style)
- Hash join: build cost + probe cost (build is startup, probe is run)
- Sort: O(n log n) total cost, all as startup cost (blocking)
- Nested loop: outer.total + outer.rows * inner.total

## Applicable to Ra

### New Rule Ideas
1. **Enforcer Sort Insertion**: When merge join needs sorted input, add Sort node
2. **Enforcer Exchange Insertion**: For distributed plans, add data movement
3. **Property-Aware Cost Extraction**: Factor ordering into e-graph extraction
4. **Multi-Phase Optimization**: Run heuristic rules first, then cost-based
5. **Aggregate Implementation Selection**: HashAgg vs SortAgg based on group count

### Gap Analysis
- Ra lacks the property tracking that Cascades provides
- Ra's egg approach handles logical equivalences well but not physical properties
- Need to augment e-graph extraction with property-aware cost function

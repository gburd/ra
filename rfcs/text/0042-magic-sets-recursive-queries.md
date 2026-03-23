# RFC 0042: Magic Sets for Recursive Queries

- Start Date: 2026-03-22
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Optimize recursive CTE (Common Table Expression) queries by pushing selections into the recursion using the Magic Sets transformation. This reduces intermediate result sizes and significantly improves performance for recursive queries with filters.

## Motivation

Recursive queries using `WITH RECURSIVE` are common for hierarchical data (org charts, bill of materials, graph traversal). Without optimization, these queries compute the entire recursive result and then apply filters, which is extremely wasteful.

**Problem Example**:
```sql
-- Find all ancestors of employee 'Bob'
WITH RECURSIVE ancestors AS (
  SELECT parent_id, child_id FROM family
  UNION ALL
  SELECT f.parent_id, a.child_id
  FROM family f JOIN ancestors a ON f.child_id = a.parent_id
)
SELECT * FROM ancestors WHERE child_id = 'Bob';
```

Current behavior:
1. Computes **full transitive closure** of family tree (potentially millions of rows)
2. Filters to child_id = 'Bob' at the end (99.99% waste)

With Magic Sets:
1. Pushes `child_id = 'Bob'` into the recursive step
2. Only expands relevant branches
3. Result: 100x-1000x faster for selective queries

### Use Cases

1. **Organizational Hierarchies**: "Find all reports under manager X"
2. **Bill of Materials**: "Find all components in product Y"
3. **Graph Reachability**: "Find all nodes reachable from node Z"
4. **Access Control**: "Find all resources user U can access via role inheritance"
5. **Dependency Analysis**: "Find all dependencies of package P"

## Guide-level explanation

Magic Sets rewrites recursive queries to propagate filter conditions from the final WHERE clause into the recursive computation. This ensures only relevant tuples are expanded.

### Before Magic Sets

```sql
WITH RECURSIVE ancestors AS (
  -- Base case: all parent-child relationships
  SELECT parent_id, child_id FROM family

  UNION ALL

  -- Recursive case: expand upwards
  SELECT f.parent_id, a.child_id
  FROM family f JOIN ancestors a ON f.child_id = a.parent_id
)
SELECT * FROM ancestors WHERE child_id = 'Bob';  -- Filter AFTER recursion
```

**Execution**:
- Iteration 1: 10,000 rows
- Iteration 2: 50,000 rows
- Iteration 3: 200,000 rows
- Final filter: 5 rows

**Total work**: 260,000 rows processed, 259,995 discarded

### After Magic Sets

```sql
WITH RECURSIVE ancestors_magic AS (
  -- Base case: ONLY rows relevant to Bob
  SELECT parent_id, child_id FROM family WHERE child_id = 'Bob'

  UNION ALL

  -- Recursive case: expand only relevant branches
  SELECT f.parent_id, a.child_id
  FROM family f JOIN ancestors_magic a ON f.child_id = a.parent_id
)
SELECT * FROM ancestors_magic;  -- No filter needed, already filtered
```

**Execution**:
- Iteration 1: 1 row (Bob's parent)
- Iteration 2: 1 row (Bob's grandparent)
- Iteration 3: 1 row (Bob's great-grandparent)
- Total: 5 rows

**Total work**: 5 rows processed, 0 discarded

### Query Patterns Supported

Magic Sets applies when:
1. Recursive CTE has a filter in outer query
2. Filter is on a column present in the recursive relation
3. Filter is equality or IN clause (not ranges/inequalities initially)

## Reference-level explanation

### Algorithm

Magic Sets transformation has three phases:

#### Phase 1: Adorned Rule Generation

1. **Identify binding patterns**: Which columns are bound (filtered) in outer query?
   - Example: `WHERE child_id = 'Bob'` -> child_id is bound
2. **Create adornment**: Mark bound columns with `b`, free columns with `f`
   - ancestors(child_id^b, parent_id^f)
3. **Generate adorned rules**: Rewrite recursive rules to propagate bindings

#### Phase 2: Magic Predicate Creation

1. **Create magic relation**: Stores bound values that need expansion
   ```sql
   CREATE TEMP TABLE magic_ancestors (child_id TEXT);
   INSERT INTO magic_ancestors VALUES ('Bob');
   ```

2. **Rewrite base case**: Join with magic relation
   ```sql
   SELECT f.parent_id, f.child_id
   FROM family f
   JOIN magic_ancestors m ON f.child_id = m.child_id
   ```

3. **Rewrite recursive case**: Join with magic relation
   ```sql
   SELECT f.parent_id, a.child_id
   FROM family f
   JOIN ancestors_magic a ON f.child_id = a.parent_id
   JOIN magic_ancestors m ON a.child_id = m.child_id
   ```

#### Phase 3: Fixpoint Computation

1. Execute adorned base case
2. Repeat recursive case until no new tuples
3. Return result (filter already applied)

### Implementation Details

**Data Structures**:

```rust
pub struct MagicSetsOptimizer {
    /// Mapping from CTE name to adorned versions
    adorned_ctes: HashMap<String, AdornedCTE>,
    /// Magic predicates for binding propagation
    magic_preds: Vec<MagicPredicate>,
}

pub struct AdornedCTE {
    /// Original CTE name
    original_name: String,
    /// Adornment pattern (e.g., "bf" for bound-free)
    adornment: String,
    /// Bound columns
    bound_columns: Vec<String>,
    /// Free columns
    free_columns: Vec<String>,
    /// Rewritten base case query
    base_case: RelExpr,
    /// Rewritten recursive case query
    recursive_case: RelExpr,
}

pub struct MagicPredicate {
    /// Name of magic relation
    magic_name: String,
    /// Columns in magic relation (bound columns from original query)
    columns: Vec<String>,
    /// Initial values (from WHERE clause)
    initial_values: Vec<Expr>,
}
```

**Transformation Steps**:

1. **Identify Recursive CTEs**: Find `RecursiveCTE` nodes in RelExpr
2. **Extract Filters**: Analyze outer query for filters on CTE columns
3. **Check Applicability**: Verify filters are on equalities/IN clauses
4. **Generate Adornment**: Mark bound vs free columns
5. **Create Magic Relations**: Materialize bound values
6. **Rewrite Base Case**: Add joins to magic relations
7. **Rewrite Recursive Case**: Propagate magic predicates
8. **Update Outer Query**: Remove redundant filter (already applied)

### Integration Points

**Interactions with**:
- **CTE Inlining**: Magic Sets should run before inlining decision
- **Join Ordering**: Magic predicates are additional relations in join graph
- **Statistics**: Magic relations have known cardinality (from filter selectivity)
- **Distributed Execution**: Magic relations can be broadcast to all nodes

**Rule Categories**:
- `rules/logical/recursive-cte-magic-sets.rra`
- `rules/logical/magic-sets-adorned-rules.rra`
- `rules/physical/magic-sets-materialization.rra`

### Error Handling

**Transformation fails if**:
1. Filter contains non-deterministic functions (RANDOM(), NOW())
2. Filter references columns not in recursive relation
3. Recursion is not linear (multiple recursive references)
4. CTE is mutually recursive with another CTE

**Fallback**: Use original unoptimized query.

### Performance Considerations

**Expected Speedup**:
- **High selectivity filters** (< 1% of data): 100x-1000x faster
- **Medium selectivity filters** (1-10% of data): 10x-50x faster
- **Low selectivity filters** (> 10% of data): 2x-5x faster
- **No filter**: Slight overhead from magic predicate tracking

**Space Overhead**:
- Magic relation size: O(bound column cardinality)
- For single-value filters (= 'Bob'): 1 row
- For IN clause filters: number of values in IN list
- Worst case: Same as original intermediate result size

**Trade-offs**:
- **Best case**: Filter eliminates 99%+ of intermediate results
- **Worst case**: Magic overhead > benefit (e.g., filter matches everything)
- **Solution**: Cost-based decision using cardinality estimates

## Drawbacks

### Complexity Cost
- Adds ~1000 lines of code for adornment analysis and rewriting
- Requires understanding of datalog semantics
- Harder to debug transformed queries

### Maintenance Burden
- New query feature: must ensure compatibility with Magic Sets
- Edge cases: nested recursion, mutually recursive CTEs
- Potential for incorrect transformations if adornment logic has bugs

### Applicability Limits
- Only helps queries with selective filters
- Does not help unfiltered recursive queries (e.g., "compute all transitive closure")
- Complex filters (OR, ranges) require extensions

### Query Rewrite Overhead
- Adornment analysis: O(query size)
- Magic predicate creation: O(number of bound columns)
- Negligible compared to query execution time

## Rationale and alternatives

### Why This Design?

**Magic Sets is proven**:
- Academic foundation: Bancilhon et al. (1986), Ullman (1989)
- Production use: Oracle, SQL Server, PostgreSQL (partial)
- Decades of refinement

**Composable with other optimizations**:
- Works alongside join reordering, filter pushdown
- Can combine with semi-join reduction in distributed setting
- Enables further optimizations (e.g., early termination)

**Cost-based applicability**:
- Can estimate benefit before transformation
- Fallback to original query if not beneficial

### Alternative Approaches

#### 1. Query Rewriting (User-Level)

**Approach**: Require users to manually rewrite queries
```sql
-- User must write this manually
WITH RECURSIVE ancestors AS (
  SELECT parent_id, child_id FROM family WHERE child_id = 'Bob'
  UNION ALL
  ...
)
```

**Pros**: No optimizer complexity
**Cons**:
- Requires expert knowledge
- Error-prone
- Doesn't work for parameterized queries (WHERE child_id = $1)

#### 2. Runtime Filtering (Dynamic)

**Approach**: Track accessed tuples during execution, prune branches dynamically

**Pros**: Adaptive to data distribution
**Cons**:
- Requires executor changes (not just optimizer)
- Higher runtime overhead
- Complex state management

#### 3. Iterative Deepening

**Approach**: Limit recursion depth, increase if needed

**Pros**: Prevents runaway recursion
**Cons**:
- Doesn't reduce work per iteration
- Multiple passes if depth guess is wrong

### Impact of Not Doing This

**Without Magic Sets**:
- Recursive queries 100x-1000x slower for selective filters
- Users avoid recursive queries (use application-level loops instead)
- Loss of declarative query capability
- Competitive disadvantage vs PostgreSQL, Oracle, SQL Server

## Prior art

### Academic Research

**"Magic Sets and Other Strange Ways to Implement Logic Programs" (Bancilhon et al., 1986)**
- Introduced Magic Sets transformation for Datalog
- Proved equivalence of adorned and original queries
- Showed order-of-magnitude performance improvements

**"Principles of Database and Knowledge-Base Systems" (Ullman, 1989)**
- Volume 2, Chapter 13: Magic Sets algorithm
- Formal semantics and correctness proofs
- Extensions: Generalized Magic Sets, Counting

**"Optimization of Recursive Queries Using Magic Sets" (Beeri & Ramakrishnan, 1991)**
- Cost-based applicability analysis
- Handling of non-linear recursion
- Integration with query optimization

### Industry Solutions

**PostgreSQL**:
- **Limited support**: Inlines simple filters into recursive term
- **Does not implement full Magic Sets**: No adornment, no magic predicates
- **Example**: `WHERE id = X` pushed into base case, but not propagated through recursion

**Oracle**:
- **CONNECT BY optimization**: Proprietary algorithm similar to Magic Sets
- **Supports**: Pruning based on START WITH and WHERE conditions
- **Performance**: 100x+ speedup for filtered hierarchical queries

**SQL Server**:
- **Recursive CTE optimization**: Partial Magic Sets implementation
- **MAXRECURSION hint**: Allows early termination
- **Execution plan**: Shows "Recursive Union" with filter pushdown

**DuckDB**:
- **Recent addition**: Basic filter pushdown into recursive CTEs
- **No full Magic Sets**: Does not create adorned rules
- **Roadmap item**: Full Magic Sets listed as future enhancement

**Apache Calcite**:
- **No support**: Recursive queries treated naively
- **Issue tracker**: Magic Sets mentioned as desired feature

### What We Can Learn

**Key insights**:
1. **Cost-based decision crucial**: Don't apply if filter has low selectivity
2. **Start with linear recursion**: Extend to non-linear later
3. **Adornment analysis is complex**: Requires datalog-style program analysis
4. **Correctness is hard**: Extensive testing needed (graph queries, corner cases)
5. **Explain plan clarity**: Users need to see "Magic Sets applied"

**DuckDB lesson**: They added simple filter pushdown first, planning full Magic Sets later. We could do the same (MVP vs full implementation).

## Unresolved questions

**Design Questions**:
1. **MVP vs Full**: Start with simple filter pushdown or full Magic Sets?
   - MVP: Push equality filters into base case only
   - Full: Adornment analysis, magic predicates, full transformation
2. **Cost model**: How to estimate benefit? Use CTE cardinality + filter selectivity?
3. **Adornment notation**: Use standard notation (^b, ^f) or custom?

**Implementation Strategy**:
1. Where in optimizer pipeline? Before or after join reordering?
2. How to materialize magic relations? Temp tables, in-memory sets, bitmaps?
3. Should we support mutually recursive CTEs initially?

**Integration Questions**:
1. Interaction with CTE inlining: Magic Sets prefers materialization
2. Distributed execution: How to broadcast magic relations?
3. EXPLAIN output: How to show transformation was applied?

**Out of Scope** (for initial RFC):
- Non-linear recursion (multiple recursive references)
- Mutually recursive CTEs
- Aggregation in recursive queries
- Complex filter predicates (OR, range queries)

## Future possibilities

### Natural Extensions

#### 1. Generalized Magic Sets
- Handle non-linear recursion
- Support mutually recursive CTEs
- Optimize recursive queries with aggregation

#### 2. Supplementary Magic Sets
- Additional optimization for queries with both bound and free access patterns
- Creates multiple adorned versions for different binding patterns

#### 3. Counting Optimization
- Specialized Magic Sets for COUNT(DISTINCT ...) over recursive queries
- Avoid materializing full result, just count

#### 4. Magic Sets for Incremental Maintenance
- Use Magic Sets to incrementally update recursive views
- When base table changes, recompute only affected recursive tuples

### Long-term Vision

**Adaptive Recursion**:
- Combine Magic Sets with runtime statistics
- Switch strategies mid-execution if heuristic was wrong
- Example: Start with Magic Sets, fall back to breadth-first if filter is non-selective

**Graph Query Integration**:
- Magic Sets as foundation for property graph queries
- MATCH (a)-[:KNOWS*]->(b) WHERE b.name = 'Alice' -> Magic Sets pushdown
- Integration with future graph query RFC

**Distributed Recursive Queries**:
- Magic Sets + semi-join reduction for multi-node execution
- Minimize data transfer in distributed graph traversal

**Cost Model Calibration**:
- Learn when Magic Sets helps via query feedback
- Build ML model: (query, filter selectivity, recursion depth) -> benefit estimate
- Auto-tune decision threshold

---

## Implementation Roadmap

### Phase 1: MVP (Simple Filter Pushdown)
- Push equality filters into base case
- No adornment analysis
- ~300 LOC
- **Benefit**: 10x-100x speedup for common cases

### Phase 2: Magic Predicates
- Full adornment analysis
- Magic relation materialization
- Propagate bindings through recursion
- ~700 LOC
- **Benefit**: 100x-1000x speedup for deep hierarchies

### Phase 3: Extensions
- Non-linear recursion
- Complex filters (IN, OR)
- Cost-based applicability
- ~500 LOC

**Total effort**: 4-6 weeks for full implementation

---

## References

- Bancilhon, F., Maier, D., Sagiv, Y., & Ullman, J. D. (1986). *Magic sets and other strange ways to implement logic programs*. PODS '86.
- Ullman, J. D. (1989). *Principles of Database and Knowledge-Base Systems, Volume 2*. Computer Science Press.
- Beeri, C., & Ramakrishnan, R. (1991). *On the power of magic*. Journal of Logic Programming.
- PostgreSQL: [Recursive Queries Documentation](https://www.postgresql.org/docs/current/queries-with.html)
- Oracle: [CONNECT BY Optimization](https://docs.oracle.com/en/database/oracle/oracle-database/19/tgsql/query-optimizer-concepts.html#GUID-7A5E0C9C-1C5E-4B7B-9F9E-3F3D3F3D3F3D)

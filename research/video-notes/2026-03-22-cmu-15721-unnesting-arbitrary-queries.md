# CMU 15-721 Lecture 14: Unnesting Arbitrary Queries (Neumann & Kemper)

**Source:** CMU 15-721 Spring 2024, Lecture 14 (Optimizer Implementation II)
**Date:** 2024-03-20
**Topic:** Advanced subquery decorrelation and unnesting techniques
**Key Papers:** "Unnesting Arbitrary Queries" (Neumann & Kemper, BTW 2015),
"Dynamic Programming Strikes Back" (Moerkotte & Neumann, SIGMOD 2008),
"The Complete Story of Joins in HyPer" (Neumann & Radke, BTW 2018)

## Key Points

This lecture covers the state-of-the-art in subquery unnesting, which is one of the
most impactful optimizations in practice because ORMs and application-generated SQL
heavily use subqueries.

### The Decorrelation Problem

Correlated subqueries force nested-loop execution: for each outer row, evaluate the
inner query. This is O(n * m) even when the inner query could be computed once.

**Classic approach (Kim 1982):** Pattern-match specific subquery forms and apply
case-specific transformations. Problem: doesn't handle arbitrary nesting.

**Neumann-Kemper approach:** General-purpose decorrelation using dependent joins
(Apply operator) as an intermediate representation.

### Dependent Join (Apply) Framework

**Step 1:** Represent correlated subquery as Apply operator:
```
Apply_type(outer, inner(outer.col))
```
Where type is: Cross, Left, Semi, Anti, Single (scalar subquery)

**Step 2:** Apply algebraic transformations to push Apply down through operators:

**Key transformation rules:**

1. **Apply through Filter:**
   ```
   Apply(R, Filter(p, S)) = Filter(p, Apply(R, S))
   ```
   When p does not reference outer columns.

2. **Apply through Project:**
   ```
   Apply(R, Project(e, S)) = Project(e, Apply(R, S))
   ```
   When projection expressions don't use outer references.

3. **Apply through Aggregate:**
   ```
   Apply(R, GroupBy(g, a, S)) = GroupBy(R.key + g, a, Apply(R, S))
   ```
   Add outer key to grouping columns to preserve per-outer-row semantics.

4. **Apply through Join:**
   ```
   Apply(R, S Join T) = Apply(R, S) Join T
   ```
   When T has no outer references. If both have references, push deeper.

5. **Apply through Union:**
   ```
   Apply(R, S UNION T) = Apply(R, S) UNION Apply(R, T)
   ```

6. **Apply elimination (base case):**
   ```
   Apply(R, S) = R Join S
   ```
   When S has no outer references (fully decorrelated).

### Handling Problematic Cases

**Scalar subqueries with aggregates:**
```sql
SELECT name, (SELECT AVG(amount) FROM orders WHERE orders.cust_id = c.id)
FROM customers c
```
Transform to:
```sql
SELECT c.name, agg.avg_amount
FROM customers c
LEFT JOIN (
  SELECT cust_id, AVG(amount) AS avg_amount FROM orders GROUP BY cust_id
) agg ON c.id = agg.cust_id
```
Key: use LEFT JOIN (not INNER) to preserve rows where subquery returns NULL.

**Multiple correlated subqueries:**
```sql
SELECT name,
  (SELECT COUNT(*) FROM orders WHERE cust_id = c.id),
  (SELECT MAX(date) FROM orders WHERE cust_id = c.id)
FROM customers c
```
Can be merged into a single join with multiple aggregates when subqueries
reference the same table with the same correlation predicate.

**Nested correlated subqueries:**
Apply decorrelation recursively from inside out.

### Dynamic Programming Strikes Back (DPccp)

For join ordering, DPccp (connected subgraph complement pair enumeration):
- Enumerate only valid join trees (connected subgraphs)
- Significantly reduces search space compared to DPsize
- Handles non-inner joins (left, semi, anti) correctly
- Prunes invalid orderings based on join type constraints

**Important for Ra:** DPccp provides the theoretical foundation for correct join
ordering with outer/semi/anti joins. Standard dynamic programming may miss valid
orderings or include invalid ones.

### The Complete Story of Joins in HyPer

Comprehensive treatment of join processing including:
- Worst-case optimal joins (WCOJ) for cyclic queries
- Adaptive join switching between hash and sort-merge at runtime
- Bloom filter passing between pipeline stages

## Optimization Rules for Ra

### New Rules Identified

1. **apply-through-aggregate** - Push Apply through GroupBy by adding outer key columns
   to the grouping set. Essential for decorrelating scalar subqueries with aggregates.

2. **apply-through-union** - Distribute Apply over UNION/UNION ALL branches, decorrelate
   each branch independently.

3. **apply-merge-same-source** - When multiple Apply operators reference the same inner
   table with the same correlation predicate, merge into a single join with multiple
   aggregate columns.

4. **left-apply-to-left-join** - Convert LeftApply (scalar subquery) to LEFT JOIN +
   aggregate, preserving NULL semantics for rows with no matching inner rows.

5. **single-apply-null-handling** - When decorrelating scalar subqueries, ensure NULL
   semantics are preserved by using COALESCE or CASE WHEN for aggregate defaults
   (COUNT returns 0, SUM returns NULL).

6. **dpccp-join-ordering** - Use connected subgraph complement pair enumeration for
   join ordering that correctly handles non-inner joins.

7. **correlated-subquery-merge** - Detect multiple correlated subqueries accessing the
   same table and merge them into a single decorrelated join.

### Ra Gap Analysis

Ra currently has extensive unnesting rules in `rules/logical/subquery-unnesting/`:
- `correlated-subquery-decorrelation.rra`
- `lateral-join-decorrelation.rra`
- `apply-to-join.rra`
- `scalar-subquery-to-join.rra`
- `subquery-with-aggregation-unnesting.rra`

**Potentially already covered:**
- Basic Apply-to-Join transformation
- Scalar subquery decorrelation
- Aggregation unnesting

**Likely missing:**
- Apply-through-aggregate with grouping key extension
- Multiple correlated subquery merging
- Apply distribution over UNION
- DPccp-based join ordering with non-inner joins
- Null-safety guarantees in scalar subquery decorrelation

## Relevance to Ra

**Priority:** High - Subquery decorrelation is one of the most impactful optimization
categories. ORM-generated SQL from Django, Rails, and SQLAlchemy heavily uses
correlated subqueries. Incomplete decorrelation can cause orders of magnitude
performance degradation.

**Action items:**
1. Verify completeness of existing decorrelation rules against the Neumann-Kemper framework
2. Add Apply-through-aggregate rule if missing
3. Add correlated subquery merging for same-source subqueries
4. Verify null-safety in scalar subquery decorrelation

# CMU 15-721 Lecture 11: Server-side Logic Execution (UDF Optimization)

**Source:** CMU 15-721 Spring 2024, Lecture 11
**Date:** 2024-03-11
**Topic:** Optimization of User-Defined Functions in relational databases
**Key Papers:** Froid (VLDB 2017), Aggify (SIGMOD 2019), Compiling PL/SQL Away (VLDB 2021)

## Key Points

The lecture covers the fundamental problem of UDF execution in relational databases: UDFs
break the optimizer's ability to reason about query plans because they create opaque
boundaries that prevent predicate pushdown, join reordering, and parallelism.

### The UDF Performance Problem

1. **Iterator-per-invocation**: Each UDF call creates a separate query execution context
2. **No cross-invocation optimization**: The optimizer cannot batch or vectorize UDF calls
3. **Context switching overhead**: Transitioning between SQL and procedural execution
4. **Parallelism barrier**: UDFs typically force serial execution of the outer query
5. **Cardinality blindness**: Optimizer cannot estimate output of UDF-containing expressions

### Froid: Imperative-to-Relational Transformation

The Froid framework (Microsoft Research) converts scalar UDFs into equivalent relational
expressions that can be inlined into the calling query:

**Transformation rules:**

1. **Assignment statements** -> Project operators
2. **IF/ELSE branches** -> CASE expressions
3. **WHILE loops** -> Recursive CTEs (when possible)
4. **Local variables** -> Derived columns in subqueries
5. **RETURN statements** -> Final projection
6. **Nested SQL queries** -> Lateral joins / Apply operators

**Example transformation:**
```sql
-- Original UDF
CREATE FUNCTION get_discount(cust_id INT) RETURNS DECIMAL AS
BEGIN
  DECLARE @tier VARCHAR(10);
  SELECT @tier = tier FROM customers WHERE id = @cust_id;
  IF @tier = 'gold' RETURN 0.20;
  IF @tier = 'silver' RETURN 0.10;
  RETURN 0.0;
END;

-- Froid-transformed inline expression
SELECT CASE
  WHEN c.tier = 'gold' THEN 0.20
  WHEN c.tier = 'silver' THEN 0.10
  ELSE 0.0
END AS discount
FROM customers c WHERE c.id = ?
```

**Performance impact:** Orders of magnitude improvement on real workloads because:
- Eliminates per-row function call overhead
- Enables predicate pushdown through the inlined expression
- Allows vectorized execution of the entire query
- Enables join reordering with UDF-referenced tables

### Aggify: Cursor Loop to Aggregate

Converts cursor-based loops into SQL aggregates:
```sql
-- Before: Cursor loop
DECLARE cur CURSOR FOR SELECT amount FROM orders WHERE cust_id = @id;
SET @total = 0;
FETCH NEXT FROM cur INTO @amt;
WHILE @@FETCH_STATUS = 0 BEGIN
  SET @total = @total + @amt;
  FETCH NEXT FROM cur INTO @amt;
END;

-- After: Single aggregate
SELECT SUM(amount) FROM orders WHERE cust_id = @id;
```

### Functional-Style UDFs

Recent work on making SQL UDFs first-class (passing functions as arguments):
- Table-valued functions as composable operators
- UDF inlining at the query plan level (not source level)
- Compilation of UDF bodies to native code alongside the query

## Optimization Rules for Ra

### New Rules Identified

1. **udf-inline-scalar** - Inline scalar UDF body into calling query as CASE/subquery
2. **udf-inline-table-valued** - Inline table-valued UDF as derived table
3. **cursor-to-aggregate** - Convert cursor iteration patterns to aggregate functions
4. **apply-to-lateral-join** - Convert APPLY operators from UDF inlining to lateral joins
5. **lateral-join-decorrelation** - Decorrelate lateral joins from UDF inlining
6. **udf-cost-estimation** - Estimate cost of UDF calls based on body complexity

### Ra Gap Analysis

Ra currently has:
- `rules/logical/subquery-unnesting/lateral-join-decorrelation.rra` - handles lateral joins
- No UDF-specific optimization rules
- No mechanism to represent or transform imperative logic

**Missing capabilities:**
- UDF body representation in the relational algebra
- Cost model for UDF execution (CPU multiplier based on complexity)
- Pattern detection for cursor-to-aggregate conversion
- Inlining decision based on UDF complexity and call frequency

## Relevance to Ra

**Priority:** Medium - UDF optimization is primarily relevant when Ra acts as an
optimizer for PostgreSQL via the pg_ra extension. PostgreSQL PL/pgSQL functions are
a major performance bottleneck in production workloads.

**Feasibility:** The lateral join decorrelation already exists. The main gap is
representing UDF bodies as relational expressions and having cost model support
for function calls.

**Proposed RFC:** Consider an RFC for UDF-aware cost estimation that assigns CPU
cost multipliers to expressions containing function calls, based on function
volatility category (IMMUTABLE, STABLE, VOLATILE) and estimated complexity.

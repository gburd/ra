# PostgreSQL v17: Self-Join Elimination

**Source:** PostgreSQL v17 Release Notes and Source Analysis
**Topic:** Self-join elimination optimization

## Key Concepts

### What is Self-Join Elimination?
A self-join is when a table is joined to itself. Many self-joins are
unnecessary and can be replaced with a single scan of the table.

### When Does It Apply?
Conditions for safe self-join elimination:
1. Both sides reference the same base table
2. Join is on a unique/primary key column
3. One side's column references are a subset of the other's
4. No conflicting filters that would change semantics
5. Currently limited to plain tables (not views or functions)

### Examples

**Before optimization:**
```sql
SELECT a.* FROM t1 a JOIN t1 b ON a.id = b.id WHERE b.x = 1;
```

**After optimization:**
```sql
SELECT * FROM t1 WHERE x = 1;
```

**More complex example:**
```sql
SELECT a.name, b.email
FROM users a
JOIN users b ON a.user_id = b.user_id
WHERE a.active = true AND b.role = 'admin';
```
->
```sql
SELECT name, email
FROM users
WHERE active = true AND role = 'admin';
```

### Implementation Details (PostgreSQL)
- Checked during join removal phase (before cost-based optimization)
- Uses unique index information to verify join is on unique key
- Replaces all column references from eliminated copy
- Merges WHERE clauses from both sides
- Only applies to INNER joins (not outer joins)

### Common Scenarios
- ORM-generated queries (multiple JOINs to same table via different paths)
- View composition (views that join to same underlying table)
- Generated SQL from reporting tools
- Complex queries built incrementally by applications

## Applicable to Ra

### New Rule
```
Rule: self-join-elimination
Pattern: Join(Inner, Scan(T, alias=A), Scan(T, alias=B), cond=A.pk = B.pk)
Result: Scan(T, alias=merged)
  where: merge predicates from both sides
  condition: join key is unique/primary key
  condition: one side's columns are subset of other
```

### Prerequisites
- Need unique key / primary key metadata in statistics
- Need column reference tracking to verify subset relationship
- Need predicate merging logic

### Impact Estimate
- Affects 5-15% of ORM-generated queries
- Eliminates entire join operation (2x or more speedup)
- No false positives (always correct when conditions met)

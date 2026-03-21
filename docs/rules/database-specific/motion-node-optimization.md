# Rule: Motion Node Optimization (Greenplum)

**Category:** database-specific/greenplum
**File:** `rules/database-specific/greenplum/motion-node-optimization.rra`

## Metadata

- **ID:** `greenplum-motion-node-optimization`
- **Version:** "1.0.0"
- **Databases:** greenplum
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Motion Node Optimization (Greenplum)

## Metadata
- **Rule ID**: `greenplum-motion-optimization`
- **Category**: Database-Specific / Greenplum
- **Source**: Greenplum (PostgreSQL MPP fork)

## Description

Greenplum minimizes data movement (Motion nodes) between segments by choosing optimal distribution strategies: Broadcast vs Redistribute vs Direct.

**Motion types:**
1. **Broadcast**: Send small table to all segments
2. **Redistribute**: Repartition by join key
3. **Direct**: No movement (co-located data)

## Relational Algebra

```
// Small table broadcast
R ⋈ S where |S| << |R|
→ Broadcast(S) ⋈ R

// Large tables redistribute
R ⋈ S where |R| ≈ |S|
→ Redistribute(R, key) ⋈ Redistribute(S, key)

// Co-located join (no motion)
R ⋈ S where distributed_by(R) = distributed_by(S) = join_key
→ Direct_Join(R, S)  // No Motion node
```

## Test Cases

### Test 1: Broadcast join
```sql
-- dim_products: 1000 rows (small)
-- fact_sales: 1B rows (large)

SELECT s.*, p.name
FROM fact_sales s
JOIN dim_products p ON s.product_id = p.id;

-- Greenplum: Broadcast dim_products to all segments
-- Each segment joins local sales with broadcasted products
-- No repartitioning of 1B row table
```

### Test 2: Redistribute join
```sql
-- orders: 100M rows
-- lineitems: 500M rows

SELECT o.*, l.*
FROM orders o
JOIN lineitems l ON o.order_id = l.order_id;

-- Greenplum: Redistribute both tables by order_id
-- Ensures matching rows co-located on same segment
```

### Test 3: Co-located join (no motion)
```sql
CREATE TABLE orders (order_id INT, ...) DISTRIBUTED BY (order_id);
CREATE TABLE lineitems (order_id INT, ...) DISTRIBUTED BY (order_id);

SELECT * FROM orders o JOIN lineitems l ON o.order_id = l.order_id;

-- No Motion node needed\!
-- Matching rows already on same segment
-- Direct local join
```

## References
1. **Greenplum Docs**: "Query Execution"
2. **Paper**: "Greenplum Database: An MPP Database for Analytics" (ITPro 2013)

## Tags
`database-specific`, `greenplum`, `mpp`, `motion`, `distributed`, `join`

# Rule: "Starburst View Merging and Unfolding"

**Category:** logical/view-rewriting
**File:** `rules/logical/view-rewriting/starburst-view-merging.rra`

## Metadata

- **ID:** `starburst-view-merging`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, mssql, oracle, db2
- **Tags:** view-merging, view-unfolding, subquery-flattening, starburst, classic
- **Authors:** "Pirahesh, Hellerstein, Hasan - IBM Starburst"


# Starburst View Merging and Unfolding

## Description

View merging (also called view unfolding or view inlining) replaces a reference
to a view with the view's defining query, then merges the resulting subquery
into the outer query. This enables the optimizer to consider cross-view
optimizations: predicate pushdown through view boundaries, join reordering
across views, and access path selection considering both view and query predicates.

Starburst's rule-based rewrite engine performs view merging as one of its first
transformation passes. The process has two phases:
1. **View unfolding**: Replace view reference with its SELECT definition
2. **Query block merging**: Combine the view's query block with the outer
   query, merging WHERE clauses, FROM clauses, and SELECT lists

Not all views can be merged. Views with DISTINCT, GROUP BY, LIMIT, UNION, or
window functions create "merge barriers" because merging would change semantics.

**When to apply**: Queries referencing views or common table expressions (CTEs)
where merging enables additional optimizations (predicate pushdown, join
reordering).

**Why it works**: Without view merging, the optimizer treats each view as an
opaque subquery that must be fully materialized before the outer query can
process it. View merging exposes the view's internal structure to the outer
query's optimizer, enabling global optimization across the view boundary.

## Relational Algebra

```algebra
View merging for simple views:

Before (view as subquery):
  pi_{a, v.b}(
    sigma_{a > 10}(
      T join_{T.id = v.tid}
        [pi_{b, tid}(sigma_{c > 5}(S)) AS v]
    )
  )

After view merge:
  pi_{a, S.b}(
    sigma_{a > 10 AND S.c > 5}(
      T join_{T.id = S.tid} S
    )
  )

The view boundary is eliminated. Now the optimizer can:
- Push a > 10 and c > 5 to their respective tables
- Consider join ordering between T and S
- Use combined statistics for cost estimation
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// View merging rewrites in Starburst style

rw!("view-merge-simple";
    "(join ?type ?pred
       ?left
       (subquery
         (project ?view_cols
           (filter ?view_pred
             (scan ?view_table)))))" =>
    "(project (merge-cols ?outer_cols ?view_cols)
       (filter (and ?pred ?view_pred)
         (join ?type ?pred ?left (scan ?view_table))))"
    if view_is_mergeable("subquery")
),

rw!("view-merge-filter-through";
    "(filter ?outer_pred
       (subquery
         (filter ?inner_pred ?inner_input)))" =>
    "(filter (and ?outer_pred ?inner_pred) ?inner_input)"
    if view_is_simple_select("subquery")
),

rw!("cte-inline";
    "(with ?cte_name ?cte_def
       (ref ?cte_name))" =>
    "?cte_def"
    if cte_used_once("?cte_name")
       && cte_is_mergeable("?cte_def")
),

// Full view merging implementation

struct ViewMerger;

impl ViewMerger {
    fn try_merge_view(
        &self,
        outer_query: &QueryBlock,
        view_ref: &ViewReference,
    ) -> Option<QueryBlock> {
        let view_def = view_ref.definition();

        // Check merge barriers
        if !self.is_mergeable(view_def) {
            return None;
        }

        // Phase 1: Unfold -- replace view reference with definition
        let mut merged = outer_query.clone();

        // Phase 2: Merge FROM clauses
        // Remove view from outer FROM, add view's FROM tables
        merged.from_clause.remove(&view_ref.alias);
        merged.from_clause.extend(
            view_def.from_clause.clone(),
        );

        // Phase 3: Merge WHERE clauses
        // Combine outer WHERE with view WHERE
        if let Some(view_where) = &view_def.where_clause {
            merged.where_clause = Some(match &merged.where_clause {
                Some(outer_where) => Predicate::And(
                    Box::new(outer_where.clone()),
                    Box::new(view_where.clone()),
                ),
                None => view_where.clone(),
            });
        }

        // Phase 4: Resolve column references
        // Replace view.col references with underlying table.col
        self.resolve_column_refs(&mut merged, view_ref, view_def);

        // Phase 5: Merge SELECT lists
        // Replace view column references in outer SELECT
        self.merge_select_lists(&mut merged, view_ref, view_def);

        Some(merged)
    }

    fn is_mergeable(&self, view_def: &QueryBlock) -> bool {
        // Merge barriers: operations that prevent flattening
        !view_def.has_distinct
            && !view_def.has_group_by
            && !view_def.has_having
            && !view_def.has_limit
            && !view_def.has_window_functions
            && !view_def.has_set_operations
            && !view_def.has_aggregation_without_groupby
            // Correlated subqueries in the view prevent merging
            && !view_def.has_correlated_subqueries
    }

    fn resolve_column_refs(
        &self,
        merged: &mut QueryBlock,
        view_ref: &ViewReference,
        view_def: &QueryBlock,
    ) {
        // Build mapping: view_alias.col -> underlying_table.col
        let mut col_map: HashMap<Column, Column> = HashMap::new();

        for (i, select_item) in view_def.select_list.iter()
            .enumerate()
        {
            let view_col = Column {
                table: view_ref.alias.clone(),
                name: select_item.alias_or_name(),
            };
            let actual_col = select_item.source_column();
            col_map.insert(view_col, actual_col);
        }

        // Replace all view column references in merged query
        merged.substitute_columns(&col_map);
    }
}
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Query references a view or CTE
    stats.has_view_references || stats.has_cte_references
        // View definition is simple enough to merge
}
```

**Restrictions:**
- Cannot merge views with: DISTINCT, GROUP BY, HAVING, LIMIT/OFFSET
- Cannot merge views with: UNION/INTERSECT/EXCEPT
- Cannot merge views with: window functions
- Cannot merge views with aggregation (SUM, COUNT, etc.) without GROUP BY
- Correlated CTEs cannot be inlined
- Multi-referenced CTEs may increase cost if inlined (query executed multiple times)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    // Benefit: cross-view optimization opportunities
    let mut benefit = 0.0;

    // Predicate pushdown through view boundary
    if stats.outer_predicate_pushable_to_view_table {
        let pushdown_selectivity =
            stats.pushable_predicate_selectivity;
        benefit += (1.0 - pushdown_selectivity) * 0.8;
    }

    // Join reordering across view boundary
    if stats.view_joins_reorderable {
        benefit += 0.3; // Typical improvement from reordering
    }

    // Index usage enabled by merged predicates
    if stats.merged_predicate_enables_index {
        benefit += 0.5;
    }

    benefit.min(10.0)
}
```

**Typical benefit**: 30% to 10x when merging enables predicate pushdown or join reordering.

## Test Cases

### Positive: Predicate pushdown through view

```sql
-- View definition
CREATE VIEW active_orders AS
  SELECT * FROM orders WHERE status = 'active';

-- Query
SELECT * FROM active_orders WHERE customer_id = 42;

-- Without merging:
-- Materialize(SELECT * FROM orders WHERE status = 'active')
-- Then filter: customer_id = 42

-- After merging:
-- SELECT * FROM orders WHERE status = 'active' AND customer_id = 42
-- Both predicates pushed to scan, index on customer_id usable!
```

### Positive: Join reordering across view boundary

```sql
CREATE VIEW customer_orders AS
  SELECT c.name, o.total
  FROM customers c JOIN orders o ON c.id = o.customer_id;

SELECT co.name, co.total, p.product_name
FROM customer_orders co
JOIN products p ON co.product_id = p.id
WHERE p.category = 'Electronics';

-- Without merging: (customers join orders) join products
-- After merging: customers join orders join products
-- Optimizer can reorder: products (filtered) join orders join customers
-- Products filtered first (most selective), smaller intermediate results
```

### Positive: CTE inlining for single-use

```sql
WITH regional_sales AS (
  SELECT region, SUM(amount) AS total
  FROM sales
  GROUP BY region
)
SELECT * FROM regional_sales WHERE total > 1000000;

-- CTE has GROUP BY -> merge barrier! Cannot inline.
-- Must materialize and filter.

-- But this CTE CAN be inlined:
WITH recent_orders AS (
  SELECT * FROM orders WHERE order_date > '2024-01-01'
)
SELECT ro.*, c.name
FROM recent_orders ro
JOIN customers c ON ro.customer_id = c.id;

-- After inlining:
-- SELECT o.*, c.name FROM orders o
-- JOIN customers c ON o.customer_id = c.id
-- WHERE o.order_date > '2024-01-01';
```

### Negative: View with GROUP BY (merge barrier)

```sql
CREATE VIEW dept_summary AS
  SELECT dept_id, COUNT(*) AS cnt, AVG(salary) AS avg_sal
  FROM employees
  GROUP BY dept_id;

SELECT * FROM dept_summary WHERE avg_sal > 100000;

-- Cannot merge: GROUP BY is a merge barrier
-- Merging would change semantics (filter before vs. after aggregation)
```

### Negative: Multi-referenced CTE (duplication risk)

```sql
WITH expensive_query AS (
  SELECT * FROM big_table WHERE complex_condition(col)
)
SELECT * FROM expensive_query e1
JOIN expensive_query e2 ON e1.id = e2.parent_id;

-- Inlining would execute expensive_query twice
-- Better to materialize once and reference twice
```

## References

**Original paper:**
- Pirahesh, H., Hellerstein, J.M., Hasan, W., "Extensible/Rule Based Query Rewrite Optimization in Starburst", ACM SIGMOD 1992
  - DOI: 10.1145/130283.130294
  - Section 3: "Query rewrite rules" -- view merging as core transformation
  - Section 3.2: "View folding/merging"

**Related work:**
- Larson, P.-A., Zhou, J., "View Matching for Outer-Join Views", VLDB 2005
  - View merging with outer joins

- Galindo-Legaria, C., Joshi, M., "Orthogonal Optimization of Subqueries and Aggregation", ACM SIGMOD 2001
  - DOI: 10.1145/375663.375746
  - Modern view merging in mssql (subquery removal)

- Elhemali, M., et al., "Execution Strategies for SQL Subqueries", ACM SIGMOD 2007
  - DOI: 10.1145/1247480.1247598
  - When to merge vs. when to materialize subqueries

**Implementation in databases:**
- PostgreSQL: `src/backend/optimizer/plan/subselect.c` - subquery flattening
- MySQL: `sql/sql_derived.cc` - derived table merging
- Oracle: "complex view merging" optimizer transformation
- mssql: "view/subquery removal" in Cascades optimizer

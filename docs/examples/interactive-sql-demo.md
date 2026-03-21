# Interactive SQL Examples

This page demonstrates Ra's interactive SQL features. You can edit, format, translate, and optimize queries directly in your browser.

## Basic Query Example

Try editing this query and clicking the buttons to format, translate to different dialects, or see the optimization plan:

```sql-interactive
SELECT c.name, COUNT(o.id) as order_count, SUM(o.amount) as total_spent
FROM customers c
JOIN orders o ON c.id = o.customer_id
WHERE o.created_at >= '2024-01-01'
GROUP BY c.id, c.name
HAVING COUNT(o.id) > 5
ORDER BY total_spent DESC
LIMIT 10
```

## Complex Join Example

This example shows a more complex query with multiple joins:

```sql-interactive
SELECT
    p.name as product_name,
    c.name as category_name,
    s.name as supplier_name,
    p.price,
    p.stock_quantity
FROM products p
INNER JOIN categories c ON p.category_id = c.id
LEFT JOIN suppliers s ON p.supplier_id = s.id
WHERE p.price > 100
    AND p.stock_quantity > 0
    AND c.active = true
ORDER BY p.price DESC, p.name ASC
```

## Window Function Example

Modern SQL with window functions for analytics:

```sql-interactive
WITH monthly_sales AS (
    SELECT
        DATE_TRUNC('month', order_date) as month,
        product_id,
        SUM(quantity * unit_price) as revenue
    FROM order_items
    WHERE order_date >= '2024-01-01'
    GROUP BY month, product_id
)
SELECT
    month,
    product_id,
    revenue,
    RANK() OVER (PARTITION BY month ORDER BY revenue DESC) as rank,
    LAG(revenue) OVER (PARTITION BY product_id ORDER BY month) as prev_month_revenue,
    revenue - LAG(revenue) OVER (PARTITION BY product_id ORDER BY month) as month_over_month_change
FROM monthly_sales
WHERE revenue > 1000
ORDER BY month, rank
```

## Features

The interactive SQL editor provides:

- **Syntax Highlighting**: Keywords, strings, and numbers are highlighted for better readability
- **Copy to Clipboard**: Quickly copy the SQL for use elsewhere
- **Format SQL**: Automatically format your query for better readability
- **Dialect Translation**: Convert queries between PostgreSQL, MySQL, SQLite, and DuckDB
- **Query Optimization**: See the query plan before and after optimization, including:
  - Cost estimates
  - Applied optimization rules
  - Execution plan visualization

## How It Works

The interactive examples use Ra's WASM module compiled from Rust, running entirely in your browser. This means:

- No server round-trips for query processing
- Instant feedback as you edit
- Full access to Ra's optimization engine
- Privacy - your queries never leave your browser

## Supported SQL Features

Ra supports a wide range of SQL features including:

- Basic SELECT, INSERT, UPDATE, DELETE
- Joins (INNER, LEFT, RIGHT, FULL, CROSS)
- Subqueries and CTEs (WITH clauses)
- Window functions
- Aggregations and GROUP BY
- Set operations (UNION, INTERSECT, EXCEPT)
- And much more!

Try experimenting with different queries to see how Ra optimizes them!
# Sample Schemas

RA Web includes five pre-loaded test schemas designed to demonstrate different query patterns and optimization scenarios.

## Overview

| Schema       | Tables | Focus Area            | Row Count Range |
|--------------|--------|-----------------------|-----------------|
| HR           | 2      | Simple joins          | 100-1,000       |
| E-Commerce   | 4      | Multi-table joins     | 1,000-10,000    |
| TPC-H        | 3      | Benchmark queries     | 10,000-100,000  |
| Sakila       | 5      | Complex relationships | 1,000-10,000    |
| Blog         | 5      | Self-joins, CTEs      | 1,000-10,000    |

All schemas are available in the Schema viewer dialog (click "Schema" button in toolbar).

## HR (Employee-Department)

### Purpose

Demonstrates basic join operations, foreign keys, and simple aggregations. Ideal for learning query optimization fundamentals.

### Schema

**employees**

```sql
CREATE TABLE employees (
  id INTEGER PRIMARY KEY,
  name VARCHAR(100) NOT NULL,
  email VARCHAR(100) UNIQUE,
  department_id INTEGER,
  salary DECIMAL(10, 2),
  hire_date DATE,
  FOREIGN KEY (department_id) REFERENCES departments(id)
);
```

**departments**

```sql
CREATE TABLE departments (
  id INTEGER PRIMARY KEY,
  name VARCHAR(100) NOT NULL,
  manager_id INTEGER,
  budget DECIMAL(12, 2),
  FOREIGN KEY (manager_id) REFERENCES employees(id)
);
```

### Relationship

- One-to-many: One department has many employees
- Self-referential: Department manager is an employee

### Sample Queries

**1. Find High Earners**

```sql
SELECT name, salary
FROM employees
WHERE salary > 100000;
```

**Demonstrates:** Simple filter, index usage on salary column

**Optimization opportunity:** Add index on salary for range queries

**2. Department Employee Count**

```sql
SELECT d.name, COUNT(e.id) as employee_count
FROM departments d
LEFT JOIN employees e ON d.id = e.department_id
GROUP BY d.id, d.name
ORDER BY employee_count DESC;
```

**Demonstrates:** LEFT JOIN (includes departments with no employees), aggregation, sorting

**Optimization opportunity:** Index on `employees.department_id`

**3. High Salary by Department**

```sql
SELECT d.name as department, e.name as employee, e.salary
FROM employees e
JOIN departments d ON e.department_id = d.id
WHERE e.salary > (SELECT AVG(salary) FROM employees)
ORDER BY e.salary DESC;
```

**Demonstrates:** Subquery, join, filter, sort

**Optimization opportunity:** Subquery might be executed once (good) or per row (bad) - check plan

## E-Commerce

### Purpose

Multi-table joins, many-to-many relationships, and business analytics queries. Represents common OLTP/reporting patterns.

### Schema

**customers** - Customer information

**orders** - Order header (one per order)

**products** - Product catalog

**order_items** - Line items (many-to-many link between orders and products)

### Relationships

- One-to-many: Customer → Orders
- One-to-many: Order → Order Items
- Many-to-many: Orders ↔ Products (via order_items)

### Sample Queries

**1. Recent Orders**

```sql
SELECT o.id, c.name, o.order_date, o.total
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.order_date > DATE('now', '-30 days')
ORDER BY o.order_date DESC;
```

**Demonstrates:** Date filtering, simple join, recency queries

**Optimization opportunity:** Composite index on `(order_date, customer_id)` for date range queries

**2. Top Products by Revenue**

```sql
SELECT p.name, SUM(oi.quantity * oi.price) as revenue
FROM order_items oi
JOIN products p ON oi.product_id = p.id
GROUP BY p.id, p.name
ORDER BY revenue DESC
LIMIT 10;
```

**Demonstrates:** Aggregation with expression, top-N query

**Optimization opportunity:** Partial aggregation, index on `order_items.product_id`

**3. Customer Order History**

```sql
SELECT
  c.name,
  COUNT(o.id) as order_count,
  SUM(o.total) as total_spent,
  MAX(o.order_date) as last_order_date
FROM customers c
LEFT JOIN orders o ON c.id = o.customer_id
GROUP BY c.id, c.name
HAVING COUNT(o.id) > 0
ORDER BY total_spent DESC;
```

**Demonstrates:** Multiple aggregations, HAVING clause, LEFT JOIN handling

**Optimization opportunity:** Covering index on orders `(customer_id, id, total, order_date)`

## TPC-H (Benchmark)

### Purpose

Industry-standard benchmark for decision support systems. Tests complex queries, large joins, and analytical workloads.

### Schema

Simplified subset of TPC-H:

**customer** - Customer master data

**orders** - Order header with dates and priorities

**lineitem** - Order line items with pricing and shipping info

### Relationships

- One-to-many: Customer → Orders
- One-to-many: Orders → Line Items

### Sample Queries

**1. Revenue by Order Priority**

```sql
SELECT o_orderpriority, COUNT(*) as order_count
FROM orders
WHERE o_orderdate >= DATE '1995-01-01'
  AND o_orderdate < DATE '1995-04-01'
GROUP BY o_orderpriority
ORDER BY o_orderpriority;
```

**Demonstrates:** Date range filtering, aggregation (TPC-H Query 4 simplified)

**Optimization opportunity:** Index on `o_orderdate` for range scans

**2. Top Customers by Revenue**

```sql
SELECT c.c_name, c.c_custkey,
       SUM(o.o_totalprice) as revenue
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
GROUP BY c.c_custkey, c.c_name
ORDER BY revenue DESC
LIMIT 10;
```

**Demonstrates:** Top-N with aggregation, join on integer keys

**Optimization opportunity:** Denormalize totals or use materialized view

**3. Shipping Analysis**

```sql
SELECT l_shipmode,
       SUM(l_quantity) as total_quantity,
       AVG(l_discount) as avg_discount
FROM lineitem
WHERE l_shipdate >= DATE '1995-01-01'
  AND l_shipdate < DATE '1996-01-01'
GROUP BY l_shipmode
ORDER BY l_shipmode;
```

**Demonstrates:** Multiple aggregations (SUM, AVG), large table scan

**Optimization opportunity:** Partition by shipdate, columnar storage

## Sakila (DVD Rental)

### Purpose

Complex many-to-many relationships, inventory tracking, and rental analysis. Models real-world video rental business.

### Schema

**film** - Movie catalog with ratings and rental rates

**actor** - Actor directory

**film_actor** - Many-to-many link (actors appear in films)

**inventory** - Physical copies of films at stores

**rental** - Rental transactions

### Relationships

- Many-to-many: Films ↔ Actors
- One-to-many: Film → Inventory
- One-to-many: Inventory → Rentals

### Sample Queries

**1. Most Popular Films**

```sql
SELECT f.title, COUNT(r.rental_id) as rental_count
FROM film f
JOIN inventory i ON f.film_id = i.film_id
JOIN rental r ON i.inventory_id = r.inventory_id
GROUP BY f.film_id, f.title
ORDER BY rental_count DESC
LIMIT 10;
```

**Demonstrates:** Three-table join, counting through relationships

**Optimization opportunity:** Materialized view for frequently queried metrics

**2. Actor Filmography**

```sql
SELECT a.first_name, a.last_name,
       COUNT(fa.film_id) as film_count
FROM actor a
JOIN film_actor fa ON a.actor_id = fa.actor_id
GROUP BY a.actor_id, a.first_name, a.last_name
HAVING COUNT(fa.film_id) > 20
ORDER BY film_count DESC;
```

**Demonstrates:** Aggregation with HAVING filter, many-to-many join

**Optimization opportunity:** Index on `film_actor.actor_id` for join

**3. Revenue by Film Rating**

```sql
SELECT f.rating,
       COUNT(*) as film_count,
       AVG(f.rental_rate) as avg_rental_rate,
       SUM(f.rental_rate) as total_revenue
FROM film f
GROUP BY f.rating
ORDER BY total_revenue DESC;
```

**Demonstrates:** Multiple aggregations, small distinct values (ratings)

**Optimization opportunity:** Very fast query (rating has low cardinality)

## Blog Platform

### Purpose

Self-referential relationships (nested comments), CTEs, recursive queries, and content management patterns.

### Schema

**users** - User accounts

**posts** - Blog posts with status (draft/published)

**comments** - Threaded comments (self-referential via parent_id)

**tags** - Tag vocabulary

**post_tags** - Many-to-many link (posts have tags)

### Relationships

- One-to-many: User → Posts
- One-to-many: Post → Comments
- Many-to-many: Posts ↔ Tags
- Self-referential: Comment → Parent Comment

### Sample Queries

**1. Recent Published Posts**

```sql
SELECT p.title, u.display_name, p.published_at
FROM posts p
JOIN users u ON p.author_id = u.id
WHERE p.status = 'published'
  AND p.published_at IS NOT NULL
ORDER BY p.published_at DESC
LIMIT 10;
```

**Demonstrates:** NULL handling, enum-like status column, recency

**Optimization opportunity:** Partial index `WHERE status = 'published'`

**2. Popular Tags**

```sql
SELECT t.name, COUNT(pt.post_id) as post_count
FROM tags t
JOIN post_tags pt ON t.id = pt.tag_id
JOIN posts p ON pt.post_id = p.id
WHERE p.status = 'published'
GROUP BY t.id, t.name
ORDER BY post_count DESC
LIMIT 10;
```

**Demonstrates:** Many-to-many aggregation, filtered join

**Optimization opportunity:** Denormalize post_count into tags table

**3. Comment Thread**

```sql
SELECT c.id, c.parent_id, c.content,
       u.display_name as author,
       c.created_at
FROM comments c
LEFT JOIN users u ON c.author_id = u.id
WHERE c.post_id = 1
  AND c.status = 'approved'
ORDER BY c.created_at;
```

**Demonstrates:** Self-referential foreign key, NULL handling (anonymous comments)

**Optimization opportunity:** Use recursive CTE to build nested tree structure

**4. Author Statistics**

```sql
SELECT u.display_name,
       COUNT(p.id) as post_count,
       COUNT(DISTINCT c.id) as comment_count,
       MAX(p.published_at) as latest_post
FROM users u
LEFT JOIN posts p ON u.id = p.author_id AND p.status = 'published'
LEFT JOIN comments c ON u.id = c.author_id AND c.status = 'approved'
GROUP BY u.id, u.display_name
HAVING COUNT(p.id) > 0
ORDER BY post_count DESC;
```

**Demonstrates:** Multiple LEFT JOINs, filtered joins, DISTINCT aggregation

**Optimization opportunity:** Separate queries (one per metric) may be faster

## Data Characteristics

### Row Counts (Approximate)

**HR**
- employees: 1,000 rows
- departments: 20 rows

**E-Commerce**
- customers: 10,000 rows
- orders: 50,000 rows
- products: 5,000 rows
- order_items: 150,000 rows

**TPC-H**
- customer: 15,000 rows
- orders: 150,000 rows
- lineitem: 600,000 rows

**Sakila**
- film: 1,000 rows
- actor: 200 rows
- film_actor: 5,000 rows
- inventory: 4,500 rows
- rental: 16,000 rows

**Blog**
- users: 1,000 rows
- posts: 5,000 rows
- comments: 20,000 rows
- tags: 100 rows
- post_tags: 15,000 rows

### Data Distribution

All test data uses realistic distributions:

- **Skewed data** - Some values much more common (Pareto distribution)
- **Temporal patterns** - Recent dates more common than old dates
- **Referential integrity** - All foreign keys valid
- **NULL values** - ~5% of nullable columns contain NULL

## Using Sample Schemas

### Loading a Sample Query

1. Click **Schema** button in toolbar

2. Select schema tab (HR, E-Commerce, etc.)

3. Click **Sample Queries** tab

4. Click any query to load it into the editor

5. Click **Execute** to run the query

### Modifying Queries

Use sample queries as starting points:

- Change WHERE conditions
- Add or remove JOINs
- Modify GROUP BY/ORDER BY
- Add LIMIT clauses
- Test different index strategies

### Exploring Tables

1. Click **Tables** tab in Schema viewer

2. Browse DDL for all tables

3. Note foreign key relationships

4. Identify potential indexes

5. Check column types and constraints

## What Each Schema Teaches

**HR** - Basics
- Simple joins
- Foreign key navigation
- Basic aggregation
- Subquery execution

**E-Commerce** - Business Logic
- Multi-table joins
- Many-to-many relationships
- Aggregation with expressions
- Top-N queries

**TPC-H** - Analytics
- Large table scans
- Date range queries
- Complex aggregations
- Benchmark comparison

**Sakila** - Complex Relationships
- Three-table joins
- Counting through relationships
- Many-to-many traversal
- Inventory tracking patterns

**Blog** - Advanced Features
- Self-referential joins
- Recursive queries (CTEs)
- Filtered joins
- Partial indexes

## Recommendations by Use Case

**Learning SQL:** Start with HR → E-Commerce → Blog

**Testing Indexes:** Use E-Commerce and TPC-H (larger datasets)

**Comparing Engines:** Use TPC-H (standardized benchmark)

**Exploring Joins:** Use Sakila (complex relationships)

**Advanced Features:** Use Blog (self-joins, CTEs, recursion)

## Tips

1. **Start simple** - Run sample queries as-is before modifying
2. **Compare engines** - Same query may have very different plans
3. **Use ANALYZE mode** - See actual performance, not just estimates
4. **Check warnings** - Learn what to look for in real queries
5. **Experiment** - Break queries to see how plans change

## Extending Sample Data

To add your own test data:

1. Create SQL files in `test-schemas/` directory
2. Mount directory in Docker Compose:
   ```yaml
   volumes:
     - ./test-schemas:/docker-entrypoint-initdb.d:ro
   ```
3. Restart containers to load data
4. Query your custom schemas via the web interface

See [Getting Started](./getting-started.md#adding-custom-schemas) for details.

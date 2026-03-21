-- SQL Performance Explained patterns by Markus Winand
-- Index-friendly and optimization-focused queries

-- Index-friendly range query
SELECT * FROM orders
WHERE order_date >= '2024-01-01' AND order_date < '2024-02-01';

-- Multi-column index usage
SELECT * FROM employees
WHERE department_id = 10 AND salary > 50000
ORDER BY last_name;

-- Avoiding function on indexed column (good)
SELECT * FROM orders
WHERE order_date >= DATE_TRUNC('month', CURRENT_DATE);

-- Composite index optimization
SELECT * FROM orders
WHERE customer_id = 123 AND order_date >= '2024-01-01'
ORDER BY order_date DESC
LIMIT 10;

-- Index-only scan candidate
SELECT employee_id, last_name, salary
FROM employees
WHERE department_id = 10
ORDER BY salary DESC;

-- Covering index usage
SELECT COUNT(*), SUM(salary), AVG(salary)
FROM employees
WHERE department_id IN (10, 20, 30);

-- Join order optimization hint via subquery
SELECT e.*, d.department_name
FROM (
    SELECT * FROM employees WHERE salary > 80000
) e
JOIN departments d ON e.department_id = d.department_id;

-- Partial index usage
SELECT * FROM orders
WHERE status = 'pending' AND order_date > CURRENT_DATE - INTERVAL '7 days';

-- Avoiding OR with UNION for better index usage
SELECT * FROM employees WHERE department_id = 10
UNION ALL
SELECT * FROM employees WHERE department_id = 20;

-- Index-friendly LIKE pattern (prefix)
SELECT * FROM products
WHERE name LIKE 'Pro%';

-- Indexed date truncation
SELECT DATE_TRUNC('month', order_date) AS month, COUNT(*), SUM(amount)
FROM orders
WHERE order_date >= '2024-01-01'
GROUP BY DATE_TRUNC('month', order_date);

-- Batch processing with LIMIT and OFFSET
SELECT * FROM large_table
WHERE processed = false
ORDER BY id
LIMIT 1000 OFFSET 0;

-- Anti-join with NOT EXISTS (often faster than NOT IN)
SELECT * FROM departments d
WHERE NOT EXISTS (
    SELECT 1 FROM employees e WHERE e.department_id = d.department_id
);

-- Lateral join for top-N per group
SELECT d.department_name, e.*
FROM departments d
CROSS JOIN LATERAL (
    SELECT * FROM employees e2
    WHERE e2.department_id = d.department_id
    ORDER BY salary DESC
    LIMIT 3
) e;

-- Partitioned outer join
SELECT d.department_name, COUNT(e.employee_id) AS emp_count
FROM departments d
LEFT JOIN employees e ON d.department_id = e.department_id
GROUP BY d.department_name;

-- Index merge candidate
SELECT * FROM employees
WHERE (department_id = 10 AND salary > 70000)
   OR (department_id = 20 AND salary > 80000);

-- Sorted merge join hint
SELECT e.*, d.*
FROM employees e
JOIN departments d ON e.department_id = d.department_id
ORDER BY e.department_id;

-- Avoiding accidental cross join
SELECT e.first_name, d.department_name
FROM employees e, departments d
WHERE e.department_id = d.department_id;

-- Bitmap index scan candidate (multiple conditions)
SELECT * FROM orders
WHERE status IN ('pending', 'processing')
  AND priority = 'high'
  AND order_date >= CURRENT_DATE - INTERVAL '30 days';

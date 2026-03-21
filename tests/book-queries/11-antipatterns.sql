-- SQL Antipatterns by Bill Karwin
-- Common patterns to avoid (but should still parse)

-- Ambiguous groups (group by primary key)
SELECT e.employee_id, e.first_name, e.last_name, d.department_name
FROM employees e
JOIN departments d ON e.department_id = d.department_id
GROUP BY e.employee_id;

-- DISTINCT as a band-aid (should work but indicates design issue)
SELECT DISTINCT e.employee_id, e.first_name
FROM employees e
JOIN job_history jh ON e.employee_id = jh.employee_id;

-- Polymorphic associations (generic foreign keys)
SELECT * FROM comments
WHERE commentable_type = 'Post' AND commentable_id = 123;

-- Entity-Attribute-Value pattern
SELECT entity_id, attribute_name, attribute_value
FROM eav_table
WHERE entity_id = 1;

-- Multicolumn attributes (storing CSV in column)
SELECT * FROM products
WHERE ',' || tags || ',' LIKE '%,electronics,%';

-- Metadata tribbles (excessive nullable columns)
SELECT employee_id, phone1, phone2, phone3, phone4, phone5
FROM employees_bad_design;

-- Fear of the unknown (NULL handling)
SELECT * FROM employees
WHERE commission_pct IS NULL OR commission_pct = 0;

-- Implicit columns with SELECT *
SELECT t1.*, t2.*
FROM table1 t1
JOIN table2 t2 ON t1.id = t2.t1_id;

-- Keyless tables (should have primary key)
SELECT * FROM logs
WHERE created_at >= CURRENT_DATE - INTERVAL '1 day';

-- Using FLOAT for currency (should use NUMERIC)
SELECT product_id, price * quantity AS total
FROM order_items;

-- Rounding errors accumulation
SELECT SUM(price * 1.08) AS total_with_tax
FROM order_items;

-- Poor use of LIKE wildcards
SELECT * FROM products
WHERE name LIKE '%widget%';

-- Readable passwords stored as plain text (example only, don't do this)
SELECT user_id, username, password_hash
FROM users
WHERE username = 'admin';

-- Index shotgun (querying without proper indexing consideration)
SELECT * FROM large_table
WHERE UPPER(name) = UPPER('John')
  AND EXTRACT(YEAR FROM created_at) = 2024;

-- Magic beans (hardcoded IDs everywhere)
SELECT * FROM orders
WHERE status_id IN (1, 2, 3, 4);

-- 31 flavors (enum overuse)
SELECT employee_id, employment_type
FROM employees
WHERE employment_type IN ('full_time', 'part_time', 'contractor', 'intern');

-- Phantom files (storing file paths in DB)
SELECT document_id, file_path
FROM documents
WHERE file_path LIKE '/uploads/%';

-- SQL injection vulnerable pattern (parameterization needed at app level)
-- This example shows the SQL syntax, not actual injection
SELECT * FROM users
WHERE username = 'input_value';

-- Incorrect date arithmetic
SELECT * FROM events
WHERE event_date = CURRENT_DATE - 7;

-- Poor NULL handling in aggregates
SELECT department_id, AVG(commission_pct)
FROM employees
GROUP BY department_id;

-- Cartesian product by mistake
SELECT e.first_name, d.department_name, l.city
FROM employees e, departments d, locations l
WHERE e.department_id = d.department_id;

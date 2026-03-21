-- Simple SELECT/WHERE/ORDER BY queries
-- Source: Common patterns from "Learning SQL" and "SQL Queries for Mere Mortals"

-- Basic SELECT
SELECT * FROM employees;

-- SELECT with specific columns
SELECT first_name, last_name, salary FROM employees;

-- WHERE clause with equality
SELECT * FROM employees WHERE department_id = 10;

-- WHERE with comparison operators
SELECT * FROM employees WHERE salary > 50000;

-- WHERE with BETWEEN
SELECT * FROM employees WHERE salary BETWEEN 40000 AND 60000;

-- WHERE with IN
SELECT * FROM employees WHERE department_id IN (10, 20, 30);

-- WHERE with LIKE pattern matching
SELECT * FROM employees WHERE last_name LIKE 'S%';

-- WHERE with multiple conditions (AND)
SELECT * FROM employees WHERE department_id = 10 AND salary > 50000;

-- WHERE with multiple conditions (OR)
SELECT * FROM employees WHERE department_id = 10 OR department_id = 20;

-- WHERE with NOT
SELECT * FROM employees WHERE NOT department_id = 10;

-- WHERE with NULL checks
SELECT * FROM employees WHERE manager_id IS NULL;

-- WHERE with NOT NULL
SELECT * FROM employees WHERE commission_pct IS NOT NULL;

-- ORDER BY single column
SELECT * FROM employees ORDER BY last_name;

-- ORDER BY descending
SELECT * FROM employees ORDER BY salary DESC;

-- ORDER BY multiple columns
SELECT * FROM employees ORDER BY department_id, last_name;

-- LIMIT clause
SELECT * FROM employees ORDER BY salary DESC LIMIT 10;

-- DISTINCT
SELECT DISTINCT department_id FROM employees;

-- Computed columns
SELECT first_name, last_name, salary * 12 AS annual_salary FROM employees;

-- String concatenation
SELECT first_name || ' ' || last_name AS full_name FROM employees;

-- CASE expressions
SELECT
    first_name,
    last_name,
    salary,
    CASE
        WHEN salary > 80000 THEN 'High'
        WHEN salary > 50000 THEN 'Medium'
        ELSE 'Low'
    END AS salary_grade
FROM employees;

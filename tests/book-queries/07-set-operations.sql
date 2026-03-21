-- Set operations (UNION, INTERSECT, EXCEPT)
-- Source: "Learning SQL", "Database System Concepts", "SQL Cookbook"

-- UNION (removes duplicates)
SELECT first_name, last_name FROM employees WHERE department_id = 10
UNION
SELECT first_name, last_name FROM employees WHERE department_id = 20;

-- UNION ALL (keeps duplicates)
SELECT first_name, last_name FROM employees WHERE department_id = 10
UNION ALL
SELECT first_name, last_name FROM employees WHERE department_id = 20;

-- Multiple UNION
SELECT first_name, last_name, 'Current' AS status FROM employees
UNION
SELECT first_name, last_name, 'Former' AS status FROM former_employees
UNION
SELECT first_name, last_name, 'Contractor' AS status FROM contractors;

-- UNION with ORDER BY
SELECT first_name, last_name FROM employees WHERE department_id = 10
UNION
SELECT first_name, last_name FROM employees WHERE department_id = 20
ORDER BY last_name;

-- INTERSECT (common rows)
SELECT employee_id FROM employees WHERE salary > 50000
INTERSECT
SELECT employee_id FROM employees WHERE department_id IN (10, 20);

-- INTERSECT with multiple columns
SELECT first_name, last_name FROM employees WHERE hire_date > '2020-01-01'
INTERSECT
SELECT first_name, last_name FROM employees WHERE salary > 60000;

-- EXCEPT (rows in first query but not in second)
SELECT employee_id FROM employees WHERE department_id = 10
EXCEPT
SELECT employee_id FROM job_history WHERE end_date IS NULL;

-- EXCEPT ALL (keeps duplicates)
SELECT department_id FROM employees
EXCEPT ALL
SELECT department_id FROM departments WHERE location_id = 1700;

-- Complex UNION with subqueries
SELECT 'High Earner' AS category, COUNT(*) AS count
FROM employees WHERE salary > 80000
UNION
SELECT 'Average Earner' AS category, COUNT(*) AS count
FROM employees WHERE salary BETWEEN 50000 AND 80000
UNION
SELECT 'Low Earner' AS category, COUNT(*) AS count
FROM employees WHERE salary < 50000;

-- UNION with aggregates
SELECT department_id, SUM(salary) AS total
FROM employees
GROUP BY department_id
UNION
SELECT NULL AS department_id, SUM(salary) AS total
FROM employees;

-- Combining set operations
(SELECT employee_id FROM employees WHERE department_id = 10
 UNION
 SELECT employee_id FROM employees WHERE department_id = 20)
INTERSECT
SELECT employee_id FROM employees WHERE salary > 60000;

-- UNION with JOINs
SELECT e.first_name, e.last_name, d.department_name
FROM employees e
JOIN departments d ON e.department_id = d.department_id
WHERE e.salary > 70000
UNION
SELECT e.first_name, e.last_name, d.department_name
FROM employees e
JOIN departments d ON e.department_id = d.department_id
WHERE e.hire_date > '2022-01-01';

-- EXCEPT to find missing values
SELECT DISTINCT department_id FROM departments
EXCEPT
SELECT DISTINCT department_id FROM employees;

-- UNION with CASE expressions
SELECT
    first_name,
    last_name,
    CASE WHEN salary > 70000 THEN 'High' ELSE 'Regular' END AS tier
FROM employees WHERE department_id = 10
UNION ALL
SELECT
    first_name,
    last_name,
    CASE WHEN salary > 70000 THEN 'High' ELSE 'Regular' END AS tier
FROM employees WHERE department_id = 20;

-- Set operation with CTEs
WITH high_salary AS (
    SELECT employee_id, first_name, last_name FROM employees WHERE salary > 80000
),
recent_hires AS (
    SELECT employee_id, first_name, last_name FROM employees WHERE hire_date > '2022-01-01'
)
SELECT * FROM high_salary
INTERSECT
SELECT * FROM recent_hires;

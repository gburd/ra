-- Aggregation queries (GROUP BY, HAVING)
-- Source: "SQL Cookbook", "Database System Concepts", "High Performance MySQL"

-- Basic COUNT
SELECT COUNT(*) FROM employees;

-- COUNT with DISTINCT
SELECT COUNT(DISTINCT department_id) FROM employees;

-- SUM, AVG, MIN, MAX
SELECT
    SUM(salary) AS total_salary,
    AVG(salary) AS avg_salary,
    MIN(salary) AS min_salary,
    MAX(salary) AS max_salary
FROM employees;

-- GROUP BY single column
SELECT department_id, COUNT(*) AS employee_count
FROM employees
GROUP BY department_id;

-- GROUP BY with multiple aggregates
SELECT
    department_id,
    COUNT(*) AS employee_count,
    AVG(salary) AS avg_salary,
    MAX(salary) AS max_salary
FROM employees
GROUP BY department_id;

-- GROUP BY multiple columns
SELECT department_id, job_id, COUNT(*) AS count
FROM employees
GROUP BY department_id, job_id;

-- HAVING clause
SELECT department_id, AVG(salary) AS avg_salary
FROM employees
GROUP BY department_id
HAVING AVG(salary) > 50000;

-- HAVING with COUNT
SELECT department_id, COUNT(*) AS employee_count
FROM employees
GROUP BY department_id
HAVING COUNT(*) > 5;

-- GROUP BY with JOIN
SELECT d.department_name, COUNT(*) AS employee_count
FROM employees e
JOIN departments d ON e.department_id = d.department_id
GROUP BY d.department_name;

-- GROUP BY with WHERE and HAVING
SELECT department_id, AVG(salary) AS avg_salary
FROM employees
WHERE hire_date > '2020-01-01'
GROUP BY department_id
HAVING AVG(salary) > 60000;

-- Multiple aggregates with CASE
SELECT
    department_id,
    COUNT(*) AS total_employees,
    COUNT(CASE WHEN salary > 70000 THEN 1 END) AS high_earners,
    COUNT(CASE WHEN salary <= 70000 THEN 1 END) AS regular_earners
FROM employees
GROUP BY department_id;

-- GROUP BY with ORDER BY on aggregate
SELECT department_id, AVG(salary) AS avg_salary
FROM employees
GROUP BY department_id
ORDER BY avg_salary DESC;

-- Aggregate with expression
SELECT
    EXTRACT(YEAR FROM hire_date) AS hire_year,
    COUNT(*) AS hires
FROM employees
GROUP BY EXTRACT(YEAR FROM hire_date);

-- String aggregation (if supported)
SELECT department_id, STRING_AGG(last_name, ', ') AS employees
FROM employees
GROUP BY department_id;

-- ROLLUP for subtotals
SELECT department_id, job_id, SUM(salary) AS total_salary
FROM employees
GROUP BY ROLLUP(department_id, job_id);

-- CUBE for all combinations
SELECT department_id, job_id, SUM(salary) AS total_salary
FROM employees
GROUP BY CUBE(department_id, job_id);

-- GROUPING SETS
SELECT department_id, job_id, SUM(salary) AS total_salary
FROM employees
GROUP BY GROUPING SETS ((department_id), (job_id), ());

-- Subquery patterns
-- Source: "SQL Cookbook", "SQL Performance Explained", "Database System Concepts"

-- Scalar subquery in SELECT
SELECT
    first_name,
    last_name,
    salary,
    (SELECT AVG(salary) FROM employees) AS avg_salary
FROM employees;

-- Subquery in WHERE with equality
SELECT * FROM employees
WHERE department_id = (SELECT department_id FROM departments WHERE department_name = 'Sales');

-- Subquery with IN
SELECT * FROM employees
WHERE department_id IN (SELECT department_id FROM departments WHERE location_id = 1700);

-- Subquery with NOT IN
SELECT * FROM employees
WHERE department_id NOT IN (SELECT department_id FROM departments WHERE location_id = 1700);

-- Subquery with EXISTS
SELECT * FROM departments d
WHERE EXISTS (SELECT 1 FROM employees e WHERE e.department_id = d.department_id);

-- Subquery with NOT EXISTS
SELECT * FROM departments d
WHERE NOT EXISTS (SELECT 1 FROM employees e WHERE e.department_id = d.department_id);

-- Correlated subquery
SELECT e1.first_name, e1.last_name, e1.salary
FROM employees e1
WHERE salary > (SELECT AVG(salary) FROM employees e2 WHERE e2.department_id = e1.department_id);

-- Subquery with ANY
SELECT * FROM employees
WHERE salary > ANY (SELECT salary FROM employees WHERE department_id = 10);

-- Subquery with ALL
SELECT * FROM employees
WHERE salary > ALL (SELECT salary FROM employees WHERE department_id = 10);

-- Subquery in FROM (derived table)
SELECT dept_avg.department_id, dept_avg.avg_salary
FROM (
    SELECT department_id, AVG(salary) AS avg_salary
    FROM employees
    GROUP BY department_id
) AS dept_avg
WHERE dept_avg.avg_salary > 50000;

-- Multiple levels of subqueries
SELECT * FROM employees
WHERE department_id IN (
    SELECT department_id FROM departments
    WHERE location_id IN (
        SELECT location_id FROM locations WHERE country_id = 'US'
    )
);

-- Subquery with JOIN
SELECT e.first_name, e.last_name, dept_info.avg_salary
FROM employees e
JOIN (
    SELECT department_id, AVG(salary) AS avg_salary
    FROM employees
    GROUP BY department_id
) AS dept_info ON e.department_id = dept_info.department_id;

-- Correlated subquery with aggregate
SELECT e1.first_name, e1.last_name
FROM employees e1
WHERE salary = (SELECT MAX(salary) FROM employees e2 WHERE e2.department_id = e1.department_id);

-- Subquery returning multiple columns
SELECT * FROM employees
WHERE (department_id, job_id) IN (
    SELECT department_id, job_id FROM job_history WHERE end_date > '2023-01-01'
);

-- Subquery with arithmetic
SELECT first_name, last_name, salary
FROM employees
WHERE salary > 1.5 * (SELECT AVG(salary) FROM employees);

-- Lateral subquery (if supported)
SELECT d.department_name, top_earner.first_name, top_earner.salary
FROM departments d,
LATERAL (
    SELECT first_name, salary
    FROM employees e
    WHERE e.department_id = d.department_id
    ORDER BY salary DESC
    LIMIT 1
) AS top_earner;

-- JOIN queries
-- Source: Common patterns from "Learning SQL", "SQL Cookbook", and "High Performance MySQL"

-- INNER JOIN
SELECT e.first_name, e.last_name, d.department_name
FROM employees e
INNER JOIN departments d ON e.department_id = d.department_id;

-- Multiple INNER JOINs
SELECT e.first_name, e.last_name, d.department_name, l.city
FROM employees e
INNER JOIN departments d ON e.department_id = d.department_id
INNER JOIN locations l ON d.location_id = l.location_id;

-- LEFT OUTER JOIN
SELECT e.first_name, e.last_name, d.department_name
FROM employees e
LEFT JOIN departments d ON e.department_id = d.department_id;

-- RIGHT OUTER JOIN
SELECT e.first_name, e.last_name, d.department_name
FROM employees e
RIGHT JOIN departments d ON e.department_id = d.department_id;

-- FULL OUTER JOIN
SELECT e.first_name, e.last_name, d.department_name
FROM employees e
FULL OUTER JOIN departments d ON e.department_id = d.department_id;

-- CROSS JOIN
SELECT e.first_name, d.department_name
FROM employees e
CROSS JOIN departments d;

-- Self-join (employee and their manager)
SELECT e.first_name AS employee, m.first_name AS manager
FROM employees e
LEFT JOIN employees m ON e.manager_id = m.employee_id;

-- JOIN with WHERE clause
SELECT e.first_name, e.last_name, d.department_name
FROM employees e
INNER JOIN departments d ON e.department_id = d.department_id
WHERE e.salary > 50000;

-- JOIN with aggregate
SELECT d.department_name, COUNT(*) AS employee_count
FROM employees e
INNER JOIN departments d ON e.department_id = d.department_id
GROUP BY d.department_name;

-- Multiple tables with complex conditions
SELECT e.first_name, e.last_name, d.department_name, j.job_title
FROM employees e
INNER JOIN departments d ON e.department_id = d.department_id
INNER JOIN jobs j ON e.job_id = j.job_id
WHERE e.salary BETWEEN 40000 AND 80000
  AND d.department_name IN ('Sales', 'Marketing');

-- Natural join (implicit column matching)
SELECT * FROM employees NATURAL JOIN departments;

-- JOIN with USING clause
SELECT e.first_name, d.department_name
FROM employees e
JOIN departments d USING (department_id);

-- Three-way join
SELECT e.first_name, d.department_name, c.country_name
FROM employees e
JOIN departments d ON e.department_id = d.department_id
JOIN locations l ON d.location_id = l.location_id
JOIN countries c ON l.country_id = c.country_id;

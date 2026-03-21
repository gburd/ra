-- Common Table Expressions (WITH clauses)
-- Source: "PostgreSQL: Up and Running", "SQL Cookbook", "T-SQL Fundamentals"

-- Simple CTE
WITH high_earners AS (
    SELECT * FROM employees WHERE salary > 80000
)
SELECT * FROM high_earners;

-- CTE with aggregation
WITH dept_stats AS (
    SELECT
        department_id,
        COUNT(*) AS employee_count,
        AVG(salary) AS avg_salary
    FROM employees
    GROUP BY department_id
)
SELECT * FROM dept_stats WHERE avg_salary > 60000;

-- Multiple CTEs
WITH
high_earners AS (
    SELECT * FROM employees WHERE salary > 80000
),
low_earners AS (
    SELECT * FROM employees WHERE salary < 40000
)
SELECT 'High' AS category, COUNT(*) AS count FROM high_earners
UNION ALL
SELECT 'Low' AS category, COUNT(*) AS count FROM low_earners;

-- CTE referencing another CTE
WITH
dept_totals AS (
    SELECT department_id, SUM(salary) AS total_salary
    FROM employees
    GROUP BY department_id
),
dept_averages AS (
    SELECT AVG(total_salary) AS avg_dept_total
    FROM dept_totals
)
SELECT * FROM dept_totals
WHERE total_salary > (SELECT avg_dept_total FROM dept_averages);

-- CTE with JOIN
WITH manager_info AS (
    SELECT employee_id, first_name || ' ' || last_name AS manager_name
    FROM employees
    WHERE employee_id IN (SELECT DISTINCT manager_id FROM employees WHERE manager_id IS NOT NULL)
)
SELECT e.first_name, e.last_name, m.manager_name
FROM employees e
JOIN manager_info m ON e.manager_id = m.employee_id;

-- Recursive CTE - employee hierarchy
WITH RECURSIVE employee_hierarchy AS (
    -- Base case: top-level managers
    SELECT employee_id, first_name, last_name, manager_id, 1 AS level
    FROM employees
    WHERE manager_id IS NULL

    UNION ALL

    -- Recursive case: employees reporting to previous level
    SELECT e.employee_id, e.first_name, e.last_name, e.manager_id, eh.level + 1
    FROM employees e
    JOIN employee_hierarchy eh ON e.manager_id = eh.employee_id
)
SELECT * FROM employee_hierarchy ORDER BY level, last_name;

-- Recursive CTE - number sequence
WITH RECURSIVE numbers AS (
    SELECT 1 AS n
    UNION ALL
    SELECT n + 1 FROM numbers WHERE n < 10
)
SELECT * FROM numbers;

-- Recursive CTE - organizational depth
WITH RECURSIVE org_depth AS (
    SELECT
        employee_id,
        first_name,
        last_name,
        manager_id,
        0 AS depth,
        CAST(first_name || ' ' || last_name AS VARCHAR(1000)) AS path
    FROM employees
    WHERE manager_id IS NULL

    UNION ALL

    SELECT
        e.employee_id,
        e.first_name,
        e.last_name,
        e.manager_id,
        od.depth + 1,
        CAST(od.path || ' > ' || e.first_name || ' ' || e.last_name AS VARCHAR(1000))
    FROM employees e
    JOIN org_depth od ON e.manager_id = od.employee_id
)
SELECT * FROM org_depth ORDER BY path;

-- CTE with window function
WITH ranked_employees AS (
    SELECT
        department_id,
        first_name,
        last_name,
        salary,
        RANK() OVER (PARTITION BY department_id ORDER BY salary DESC) AS dept_rank
    FROM employees
)
SELECT * FROM ranked_employees WHERE dept_rank <= 3;

-- CTE in UPDATE (if supported)
WITH top_performers AS (
    SELECT employee_id
    FROM employees
    WHERE salary > 100000
)
UPDATE employees
SET bonus = salary * 0.1
WHERE employee_id IN (SELECT employee_id FROM top_performers);

-- CTE in DELETE (if supported)
WITH inactive_departments AS (
    SELECT d.department_id
    FROM departments d
    LEFT JOIN employees e ON d.department_id = e.department_id
    WHERE e.employee_id IS NULL
)
DELETE FROM departments
WHERE department_id IN (SELECT department_id FROM inactive_departments);

-- Recursive CTE - Bill of Materials
WITH RECURSIVE parts_tree AS (
    SELECT
        part_id,
        part_name,
        parent_part_id,
        1 AS level,
        CAST(part_name AS VARCHAR(1000)) AS path
    FROM parts
    WHERE parent_part_id IS NULL

    UNION ALL

    SELECT
        p.part_id,
        p.part_name,
        p.parent_part_id,
        pt.level + 1,
        CAST(pt.path || ' > ' || p.part_name AS VARCHAR(1000))
    FROM parts p
    JOIN parts_tree pt ON p.parent_part_id = pt.part_id
)
SELECT * FROM parts_tree;

-- CTE with MATERIALIZED hint (PostgreSQL)
WITH dept_summary AS MATERIALIZED (
    SELECT
        department_id,
        COUNT(*) AS employee_count,
        AVG(salary) AS avg_salary
    FROM employees
    GROUP BY department_id
)
SELECT d.department_name, ds.employee_count, ds.avg_salary
FROM departments d
JOIN dept_summary ds ON d.department_id = ds.department_id;

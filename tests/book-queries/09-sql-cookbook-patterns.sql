-- SQL Cookbook patterns by Anthony Molinaro
-- Common real-world query patterns

-- Pivoting results (rows to columns)
SELECT
    department_id,
    COUNT(CASE WHEN salary < 50000 THEN 1 END) AS low_salary,
    COUNT(CASE WHEN salary BETWEEN 50000 AND 100000 THEN 1 END) AS mid_salary,
    COUNT(CASE WHEN salary > 100000 THEN 1 END) AS high_salary
FROM employees
GROUP BY department_id;

-- Unpivoting (columns to rows) using UNION
SELECT employee_id, 'Q1' AS quarter, q1_sales AS sales FROM quarterly_sales
UNION ALL
SELECT employee_id, 'Q2' AS quarter, q2_sales AS sales FROM quarterly_sales
UNION ALL
SELECT employee_id, 'Q3' AS quarter, q3_sales AS sales FROM quarterly_sales
UNION ALL
SELECT employee_id, 'Q4' AS quarter, q4_sales AS sales FROM quarterly_sales;

-- Finding duplicate rows
SELECT first_name, last_name, COUNT(*) AS duplicate_count
FROM employees
GROUP BY first_name, last_name
HAVING COUNT(*) > 1;

-- Deleting duplicates (keep one)
DELETE FROM employees
WHERE employee_id NOT IN (
    SELECT MIN(employee_id)
    FROM employees
    GROUP BY first_name, last_name, email
);

-- Generating running totals by group
SELECT
    department_id,
    employee_id,
    salary,
    SUM(salary) OVER (PARTITION BY department_id ORDER BY employee_id) AS running_total
FROM employees;

-- Finding rows with maximum value per group
SELECT e1.*
FROM employees e1
WHERE salary = (
    SELECT MAX(salary)
    FROM employees e2
    WHERE e2.department_id = e1.department_id
);

-- Finding missing sequence values
WITH nums AS (
    SELECT generate_series(1, 1000) AS num
)
SELECT num
FROM nums
WHERE num NOT IN (SELECT employee_id FROM employees);

-- Ranking with ties handled
SELECT
    first_name,
    last_name,
    salary,
    DENSE_RANK() OVER (ORDER BY salary DESC) AS salary_rank,
    CASE
        WHEN DENSE_RANK() OVER (ORDER BY salary DESC) <= 10 THEN 'Top 10'
        ELSE 'Other'
    END AS tier
FROM employees;

-- Cumulative aggregations
SELECT
    order_date,
    order_id,
    amount,
    SUM(amount) OVER (ORDER BY order_date) AS cumulative_revenue,
    COUNT(*) OVER (ORDER BY order_date) AS cumulative_orders,
    AVG(amount) OVER (ORDER BY order_date) AS cumulative_avg
FROM orders;

-- Finding overlapping time ranges
SELECT t1.employee_id, t1.project_id AS project1, t2.project_id AS project2
FROM time_tracking t1
JOIN time_tracking t2 ON t1.employee_id = t2.employee_id
    AND t1.project_id < t2.project_id
    AND t1.start_time < t2.end_time
    AND t1.end_time > t2.start_time;

-- Comparing adjacent rows
SELECT
    order_id,
    order_date,
    amount,
    LAG(amount) OVER (ORDER BY order_date) AS prev_amount,
    amount - LAG(amount) OVER (ORDER BY order_date) AS change_from_prev
FROM orders;

-- Median calculation using percentile
SELECT
    department_id,
    PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY salary) AS median_salary
FROM employees
GROUP BY department_id;

-- First and last value per group
SELECT DISTINCT
    department_id,
    FIRST_VALUE(employee_id) OVER (PARTITION BY department_id ORDER BY hire_date) AS first_hire,
    LAST_VALUE(employee_id) OVER (
        PARTITION BY department_id
        ORDER BY hire_date
        ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
    ) AS last_hire
FROM employees;

-- Pattern matching for complex filters
SELECT * FROM products
WHERE name ~ '^[A-Z]{3}-[0-9]{4}$'  -- Regex pattern
   OR name LIKE '%Pro%'
   OR name SIMILAR TO '%(Pro|Premium|Plus)%';

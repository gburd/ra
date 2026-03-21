-- Window function queries
-- Source: "SQL Performance Explained", "PostgreSQL: Up and Running", "T-SQL Fundamentals"

-- ROW_NUMBER
SELECT
    first_name,
    last_name,
    salary,
    ROW_NUMBER() OVER (ORDER BY salary DESC) AS salary_rank
FROM employees;

-- ROW_NUMBER with PARTITION BY
SELECT
    department_id,
    first_name,
    last_name,
    salary,
    ROW_NUMBER() OVER (PARTITION BY department_id ORDER BY salary DESC) AS dept_rank
FROM employees;

-- RANK (with gaps for ties)
SELECT
    first_name,
    last_name,
    salary,
    RANK() OVER (ORDER BY salary DESC) AS salary_rank
FROM employees;

-- DENSE_RANK (no gaps for ties)
SELECT
    first_name,
    last_name,
    salary,
    DENSE_RANK() OVER (ORDER BY salary DESC) AS salary_rank
FROM employees;

-- NTILE (divide into N groups)
SELECT
    first_name,
    last_name,
    salary,
    NTILE(4) OVER (ORDER BY salary DESC) AS salary_quartile
FROM employees;

-- LAG (access previous row)
SELECT
    first_name,
    last_name,
    hire_date,
    salary,
    LAG(salary) OVER (ORDER BY hire_date) AS prev_salary
FROM employees;

-- LEAD (access next row)
SELECT
    first_name,
    last_name,
    salary,
    LEAD(salary) OVER (ORDER BY salary) AS next_salary
FROM employees;

-- LAG with default value
SELECT
    first_name,
    last_name,
    salary,
    LAG(salary, 1, 0) OVER (ORDER BY hire_date) AS prev_salary
FROM employees;

-- FIRST_VALUE
SELECT
    department_id,
    first_name,
    last_name,
    salary,
    FIRST_VALUE(salary) OVER (PARTITION BY department_id ORDER BY salary DESC) AS highest_salary
FROM employees;

-- LAST_VALUE with proper frame
SELECT
    department_id,
    first_name,
    last_name,
    salary,
    LAST_VALUE(salary) OVER (
        PARTITION BY department_id
        ORDER BY salary DESC
        ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
    ) AS lowest_salary
FROM employees;

-- SUM as window function
SELECT
    department_id,
    first_name,
    last_name,
    salary,
    SUM(salary) OVER (PARTITION BY department_id) AS dept_total_salary
FROM employees;

-- Running total
SELECT
    first_name,
    last_name,
    hire_date,
    salary,
    SUM(salary) OVER (ORDER BY hire_date ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS running_total
FROM employees;

-- AVG as window function
SELECT
    department_id,
    first_name,
    last_name,
    salary,
    AVG(salary) OVER (PARTITION BY department_id) AS dept_avg_salary
FROM employees;

-- Moving average
SELECT
    first_name,
    last_name,
    hire_date,
    salary,
    AVG(salary) OVER (ORDER BY hire_date ROWS BETWEEN 2 PRECEDING AND CURRENT ROW) AS moving_avg_3
FROM employees;

-- COUNT as window function
SELECT
    department_id,
    first_name,
    last_name,
    COUNT(*) OVER (PARTITION BY department_id) AS dept_count
FROM employees;

-- Multiple window functions
SELECT
    department_id,
    first_name,
    last_name,
    salary,
    ROW_NUMBER() OVER (PARTITION BY department_id ORDER BY salary DESC) AS dept_rank,
    RANK() OVER (ORDER BY salary DESC) AS overall_rank,
    AVG(salary) OVER (PARTITION BY department_id) AS dept_avg
FROM employees;

-- Named window
SELECT
    department_id,
    first_name,
    last_name,
    salary,
    ROW_NUMBER() OVER w AS dept_rank,
    AVG(salary) OVER w AS dept_avg
FROM employees
WINDOW w AS (PARTITION BY department_id ORDER BY salary DESC);

-- RANGE frame
SELECT
    first_name,
    last_name,
    salary,
    SUM(salary) OVER (ORDER BY salary RANGE BETWEEN 1000 PRECEDING AND 1000 FOLLOWING) AS salary_range_sum
FROM employees;

-- CUME_DIST (cumulative distribution)
SELECT
    first_name,
    last_name,
    salary,
    CUME_DIST() OVER (ORDER BY salary) AS cumulative_dist
FROM employees;

-- PERCENT_RANK
SELECT
    first_name,
    last_name,
    salary,
    PERCENT_RANK() OVER (ORDER BY salary) AS pct_rank
FROM employees;

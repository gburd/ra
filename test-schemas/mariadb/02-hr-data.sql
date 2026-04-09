-- HR Schema Data Generation for MariaDB

-- Generate departments
INSERT INTO departments (dept_name, location, budget)
WITH RECURSIVE nums AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM nums WHERE n < 100
)
SELECT
  CONCAT('Department ', n),
  ELT((n % 8) + 1, 'San Francisco', 'New York', 'Austin', 'Chicago', 'Seattle', 'Boston', 'Denver', 'Portland'),
  500000 + FLOOR(RAND(n) * 4500000)
FROM nums;

-- Generate 10,000 employees
INSERT INTO employees (name, department_id, salary, hire_date, email)
WITH RECURSIVE nums AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM nums WHERE n < 10000
)
SELECT
  CONCAT('Employee ', n),
  ((n - 1) % 100) + 1,
  30000 + FLOOR(RAND(n) * 170000),
  DATE_SUB(CURDATE(), INTERVAL FLOOR(RAND(n * 2) * 3650) DAY),
  CONCAT('employee', n, '@company.com')
FROM nums;

-- Create indexes
CREATE INDEX idx_employees_department_id ON employees(department_id);
CREATE INDEX idx_employees_salary ON employees(salary);
CREATE INDEX idx_employees_hire_date ON employees(hire_date);

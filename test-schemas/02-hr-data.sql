-- HR Schema Data Generation
-- Generates 100 departments and 10,000 employees

\echo 'Generating HR test data...'

-- Generate 100 departments
INSERT INTO departments (dept_name, location)
SELECT
  'Department ' || i,
  (ARRAY['San Francisco', 'New York', 'Austin', 'Chicago', 'Seattle', 'Boston', 'Denver', 'Portland'])[1 + (i % 8)]
FROM generate_series(1, 100) AS i;

\echo 'Generated 100 departments'

-- Generate 10,000 employees
INSERT INTO employees (name, department_id, salary, hire_date)
SELECT
  'Employee ' || i,
  1 + (i % 100),
  30000 + (random() * 170000)::int,
  CURRENT_DATE - (random() * 3650)::int
FROM generate_series(1, 10000) AS i;

\echo 'Generated 10,000 employees'

-- Create indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_employees_department_id ON employees(department_id);
CREATE INDEX IF NOT EXISTS idx_employees_salary ON employees(salary);
CREATE INDEX IF NOT EXISTS idx_employees_hire_date ON employees(hire_date);

\echo 'HR test data generation complete'

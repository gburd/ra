-- HR (Employee-Department) Schema
-- Demonstrates JOIN optimizations, aggregations, and filtering

-- Create departments table
CREATE TABLE departments (
    dept_id SERIAL PRIMARY KEY,
    dept_name VARCHAR(100) NOT NULL,
    location VARCHAR(100) NOT NULL,
    budget DECIMAL(12,2) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create employees table
CREATE TABLE employees (
    emp_id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    department_id INTEGER REFERENCES departments(dept_id),
    salary DECIMAL(10,2) NOT NULL,
    hire_date DATE NOT NULL,
    email VARCHAR(100) UNIQUE,
    manager_id INTEGER REFERENCES employees(emp_id),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes
CREATE INDEX idx_employees_dept ON employees(department_id);
CREATE INDEX idx_employees_salary ON employees(salary);
CREATE INDEX idx_employees_hire_date ON employees(hire_date);
CREATE INDEX idx_employees_manager ON employees(manager_id);

-- Insert sample departments (realistic distribution)
INSERT INTO departments (dept_name, location, budget) VALUES
    ('Engineering', 'San Francisco', 5000000.00),
    ('Sales', 'New York', 3000000.00),
    ('Marketing', 'Los Angeles', 2000000.00),
    ('HR', 'Chicago', 1000000.00),
    ('Finance', 'Boston', 1500000.00),
    ('Operations', 'Seattle', 2500000.00),
    ('Customer Support', 'Austin', 1200000.00),
    ('Product', 'San Francisco', 3500000.00),
    ('Legal', 'New York', 800000.00),
    ('Research', 'San Francisco', 4000000.00);

-- Insert sample employees (100 employees with realistic distributions)
-- Engineering (dept_id=1) - 25 employees
INSERT INTO employees (name, department_id, salary, hire_date, email, manager_id) VALUES
    ('Alice Johnson', 1, 180000.00, '2018-01-15', 'alice.johnson@company.com', NULL),
    ('Bob Smith', 1, 160000.00, '2019-03-20', 'bob.smith@company.com', 1),
    ('Carol Williams', 1, 155000.00, '2019-06-10', 'carol.williams@company.com', 1),
    ('David Brown', 1, 145000.00, '2020-01-05', 'david.brown@company.com', 2),
    ('Eve Davis', 1, 140000.00, '2020-02-15', 'eve.davis@company.com', 2),
    ('Frank Miller', 1, 135000.00, '2020-05-20', 'frank.miller@company.com', 3),
    ('Grace Wilson', 1, 130000.00, '2020-08-10', 'grace.wilson@company.com', 3),
    ('Henry Moore', 1, 125000.00, '2021-01-12', 'henry.moore@company.com', 2),
    ('Iris Taylor', 1, 120000.00, '2021-03-15', 'iris.taylor@company.com', 2),
    ('Jack Anderson', 1, 115000.00, '2021-06-20', 'jack.anderson@company.com', 3),
    ('Kate Thomas', 1, 110000.00, '2021-09-10', 'kate.thomas@company.com', 3),
    ('Leo Jackson', 1, 105000.00, '2022-01-05', 'leo.jackson@company.com', 4),
    ('Mary White', 1, 100000.00, '2022-03-15', 'mary.white@company.com', 4),
    ('Noah Harris', 1, 95000.00, '2022-06-20', 'noah.harris@company.com', 5),
    ('Olivia Martin', 1, 90000.00, '2022-09-10', 'olivia.martin@company.com', 5),
    ('Paul Thompson', 1, 85000.00, '2023-01-12', 'paul.thompson@company.com', 4),
    ('Quinn Garcia', 1, 85000.00, '2023-03-15', 'quinn.garcia@company.com', 4),
    ('Rachel Martinez', 1, 80000.00, '2023-06-20', 'rachel.martinez@company.com', 5),
    ('Sam Robinson', 1, 80000.00, '2023-09-10', 'sam.robinson@company.com', 5),
    ('Tina Clark', 1, 75000.00, '2024-01-05', 'tina.clark@company.com', 6),
    ('Uma Rodriguez', 1, 75000.00, '2024-03-15', 'uma.rodriguez@company.com', 6),
    ('Victor Lewis', 1, 70000.00, '2024-06-20', 'victor.lewis@company.com', 7),
    ('Wendy Lee', 1, 70000.00, '2024-09-10', 'wendy.lee@company.com', 7),
    ('Xavier Walker', 1, 65000.00, '2025-01-12', 'xavier.walker@company.com', 6),
    ('Yara Hall', 1, 65000.00, '2025-03-15', 'yara.hall@company.com', 7);

-- Sales (dept_id=2) - 20 employees
INSERT INTO employees (name, department_id, salary, hire_date, email, manager_id) VALUES
    ('Zack Allen', 2, 150000.00, '2018-02-10', 'zack.allen@company.com', NULL),
    ('Amy Young', 2, 130000.00, '2019-04-15', 'amy.young@company.com', 26),
    ('Brian King', 2, 125000.00, '2019-07-20', 'brian.king@company.com', 26),
    ('Cindy Wright', 2, 120000.00, '2020-02-10', 'cindy.wright@company.com', 27),
    ('Derek Lopez', 2, 115000.00, '2020-05-15', 'derek.lopez@company.com', 27),
    ('Emily Hill', 2, 110000.00, '2020-08-20', 'emily.hill@company.com', 28),
    ('Fred Scott', 2, 105000.00, '2021-02-10', 'fred.scott@company.com', 28),
    ('Gina Green', 2, 100000.00, '2021-05-15', 'gina.green@company.com', 27),
    ('Harry Adams', 2, 95000.00, '2021-08-20', 'harry.adams@company.com', 28),
    ('Ivy Baker', 2, 90000.00, '2022-02-10', 'ivy.baker@company.com', 29),
    ('James Gonzalez', 2, 85000.00, '2022-05-15', 'james.gonzalez@company.com', 29),
    ('Kelly Nelson', 2, 80000.00, '2022-08-20', 'kelly.nelson@company.com', 30),
    ('Larry Carter', 2, 80000.00, '2023-02-10', 'larry.carter@company.com', 30),
    ('Megan Mitchell', 2, 75000.00, '2023-05-15', 'megan.mitchell@company.com', 31),
    ('Nick Perez', 2, 75000.00, '2023-08-20', 'nick.perez@company.com', 31),
    ('Opal Roberts', 2, 70000.00, '2024-02-10', 'opal.roberts@company.com', 32),
    ('Pete Turner', 2, 70000.00, '2024-05-15', 'pete.turner@company.com', 32),
    ('Quincy Phillips', 2, 65000.00, '2024-08-20', 'quincy.phillips@company.com', 33),
    ('Rose Campbell', 2, 65000.00, '2025-02-10', 'rose.campbell@company.com', 33),
    ('Steve Parker', 2, 60000.00, '2025-05-15', 'steve.parker@company.com', 34);

-- Marketing (dept_id=3) - 15 employees
INSERT INTO employees (name, department_id, salary, hire_date, email, manager_id) VALUES
    ('Tracy Evans', 3, 140000.00, '2018-03-15', 'tracy.evans@company.com', NULL),
    ('Uma Edwards', 3, 120000.00, '2019-05-20', 'uma.edwards@company.com', 46),
    ('Victor Collins', 3, 115000.00, '2019-08-25', 'victor.collins@company.com', 46),
    ('Wendy Stewart', 3, 110000.00, '2020-03-15', 'wendy.stewart@company.com', 47),
    ('Xavier Sanchez', 3, 105000.00, '2020-06-20', 'xavier.sanchez@company.com', 47),
    ('Yvonne Morris', 3, 100000.00, '2020-09-25', 'yvonne.morris@company.com', 48),
    ('Zane Rogers', 3, 95000.00, '2021-03-15', 'zane.rogers@company.com', 48),
    ('Anna Reed', 3, 90000.00, '2021-06-20', 'anna.reed@company.com', 47),
    ('Barry Cook', 3, 85000.00, '2021-09-25', 'barry.cook@company.com', 48),
    ('Carla Morgan', 3, 80000.00, '2022-03-15', 'carla.morgan@company.com', 49),
    ('Dan Bell', 3, 75000.00, '2022-06-20', 'dan.bell@company.com', 49),
    ('Ella Murphy', 3, 70000.00, '2022-09-25', 'ella.murphy@company.com', 50),
    ('Frank Bailey', 3, 70000.00, '2023-03-15', 'frank.bailey@company.com', 50),
    ('Gloria Rivera', 3, 65000.00, '2023-06-20', 'gloria.rivera@company.com', 51),
    ('Hank Cooper', 3, 65000.00, '2023-09-25', 'hank.cooper@company.com', 51);

-- HR (dept_id=4) - 10 employees
INSERT INTO employees (name, department_id, salary, hire_date, email, manager_id) VALUES
    ('Irene Richardson', 4, 130000.00, '2018-04-20', 'irene.richardson@company.com', NULL),
    ('Jack Cox', 4, 110000.00, '2019-06-25', 'jack.cox@company.com', 61),
    ('Karen Howard', 4, 105000.00, '2019-09-30', 'karen.howard@company.com', 61),
    ('Leo Ward', 4, 100000.00, '2020-04-20', 'leo.ward@company.com', 62),
    ('Mary Torres', 4, 95000.00, '2020-07-25', 'mary.torres@company.com', 62),
    ('Noah Peterson', 4, 90000.00, '2020-10-30', 'noah.peterson@company.com', 63),
    ('Olivia Gray', 4, 85000.00, '2021-04-20', 'olivia.gray@company.com', 63),
    ('Paul Ramirez', 4, 80000.00, '2021-07-25', 'paul.ramirez@company.com', 62),
    ('Quinn James', 4, 75000.00, '2021-10-30', 'quinn.james@company.com', 63),
    ('Rachel Watson', 4, 70000.00, '2022-04-20', 'rachel.watson@company.com', 64);

-- Finance (dept_id=5) - 12 employees
INSERT INTO employees (name, department_id, salary, hire_date, email, manager_id) VALUES
    ('Sam Brooks', 5, 160000.00, '2018-05-25', 'sam.brooks@company.com', NULL),
    ('Tina Kelly', 5, 140000.00, '2019-07-30', 'tina.kelly@company.com', 71),
    ('Uma Sanders', 5, 135000.00, '2019-10-05', 'uma.sanders@company.com', 71),
    ('Victor Price', 5, 130000.00, '2020-05-25', 'victor.price@company.com', 72),
    ('Wendy Bennett', 5, 125000.00, '2020-08-30', 'wendy.bennett@company.com', 72),
    ('Xavier Wood', 5, 120000.00, '2020-11-05', 'xavier.wood@company.com', 73),
    ('Yara Barnes', 5, 115000.00, '2021-05-25', 'yara.barnes@company.com', 73),
    ('Zack Ross', 5, 110000.00, '2021-08-30', 'zack.ross@company.com', 72),
    ('Amy Henderson', 5, 105000.00, '2021-11-05', 'amy.henderson@company.com', 73),
    ('Brian Coleman', 5, 100000.00, '2022-05-25', 'brian.coleman@company.com', 74),
    ('Cindy Jenkins', 5, 95000.00, '2022-08-30', 'cindy.jenkins@company.com', 74),
    ('Derek Perry', 5, 90000.00, '2022-11-05', 'derek.perry@company.com', 75);

-- Operations (dept_id=6) - 10 employees
INSERT INTO employees (name, department_id, salary, hire_date, email, manager_id) VALUES
    ('Emily Powell', 6, 145000.00, '2018-06-30', 'emily.powell@company.com', NULL),
    ('Fred Long', 6, 125000.00, '2019-08-05', 'fred.long@company.com', 83),
    ('Gina Patterson', 6, 120000.00, '2019-11-10', 'gina.patterson@company.com', 83),
    ('Harry Hughes', 6, 115000.00, '2020-06-30', 'harry.hughes@company.com', 84),
    ('Ivy Flores', 6, 110000.00, '2020-09-05', 'ivy.flores@company.com', 84),
    ('James Washington', 6, 105000.00, '2020-12-10', 'james.washington@company.com', 85),
    ('Kelly Butler', 6, 100000.00, '2021-06-30', 'kelly.butler@company.com', 85),
    ('Larry Simmons', 6, 95000.00, '2021-09-05', 'larry.simmons@company.com', 84),
    ('Megan Foster', 6, 90000.00, '2021-12-10', 'megan.foster@company.com', 85),
    ('Nick Gonzales', 6, 85000.00, '2022-06-30', 'nick.gonzales@company.com', 86);

-- Customer Support (dept_id=7) - 8 employees
INSERT INTO employees (name, department_id, salary, hire_date, email, manager_id) VALUES
    ('Opal Bryant', 7, 110000.00, '2019-01-10', 'opal.bryant@company.com', NULL),
    ('Pete Alexander', 7, 95000.00, '2019-08-15', 'pete.alexander@company.com', 93),
    ('Quincy Russell', 7, 90000.00, '2020-01-10', 'quincy.russell@company.com', 93),
    ('Rose Griffin', 7, 85000.00, '2020-08-15', 'rose.griffin@company.com', 94),
    ('Steve Diaz', 7, 80000.00, '2021-01-10', 'steve.diaz@company.com', 94),
    ('Tracy Hayes', 7, 75000.00, '2021-08-15', 'tracy.hayes@company.com', 95),
    ('Uma Myers', 7, 70000.00, '2022-01-10', 'uma.myers@company.com', 95),
    ('Victor Ford', 7, 65000.00, '2022-08-15', 'victor.ford@company.com', 96);

-- Analyze tables for optimal query planning
ANALYZE departments;
ANALYZE employees;

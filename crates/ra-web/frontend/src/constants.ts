import type { Engine, EngineConfig, Schema } from './types';

export const ENGINES: EngineConfig[] = [
  { id: 'postgresql-15', name: 'PostgreSQL', version: '15' },
  { id: 'postgresql-16', name: 'PostgreSQL', version: '16' },
  { id: 'postgresql-17', name: 'PostgreSQL', version: '17' },
  { id: 'mysql-8.0', name: 'MySQL', version: '8.0' },
  { id: 'mysql-8.4', name: 'MySQL', version: '8.4' },
  { id: 'duckdb', name: 'DuckDB', version: 'Latest' },
  { id: 'sqlite', name: 'SQLite', version: 'Latest' },
];

export const DEFAULT_ENGINE: Engine = 'postgresql-16';

export const QUERY_TIMEOUT_MS = 30000;

export const DEFAULT_SQL = `-- Enter your SQL query here
SELECT * FROM employees
WHERE department_id = 1;`;

export const SCHEMAS: Schema[] = [
  {
    name: 'HR (Employee-Department)',
    tables: [
      {
        name: 'employees',
        ddl: `CREATE TABLE employees (
  id INTEGER PRIMARY KEY,
  name VARCHAR(100) NOT NULL,
  email VARCHAR(100) UNIQUE,
  department_id INTEGER,
  salary DECIMAL(10, 2),
  hire_date DATE,
  FOREIGN KEY (department_id) REFERENCES departments(id)
);`,
      },
      {
        name: 'departments',
        ddl: `CREATE TABLE departments (
  id INTEGER PRIMARY KEY,
  name VARCHAR(100) NOT NULL,
  manager_id INTEGER,
  budget DECIMAL(12, 2),
  FOREIGN KEY (manager_id) REFERENCES employees(id)
);`,
      },
    ],
    sampleQueries: [
      {
        name: 'Find High Earners',
        sql: 'SELECT name, salary FROM employees WHERE salary > 100000;',
        description: 'Simple filter on salary',
      },
      {
        name: 'Department Employee Count',
        sql: `SELECT d.name, COUNT(e.id) as employee_count
FROM departments d
LEFT JOIN employees e ON d.id = e.department_id
GROUP BY d.id, d.name
ORDER BY employee_count DESC;`,
        description: 'Join with aggregation',
      },
      {
        name: 'High Salary by Department',
        sql: `SELECT d.name as department, e.name as employee, e.salary
FROM employees e
JOIN departments d ON e.department_id = d.id
WHERE e.salary > (SELECT AVG(salary) FROM employees)
ORDER BY e.salary DESC;`,
        description: 'Join with subquery',
      },
    ],
  },
  {
    name: 'E-Commerce',
    tables: [
      {
        name: 'customers',
        ddl: `CREATE TABLE customers (
  id INTEGER PRIMARY KEY,
  name VARCHAR(100) NOT NULL,
  email VARCHAR(100) UNIQUE,
  city VARCHAR(50),
  country VARCHAR(50),
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);`,
      },
      {
        name: 'orders',
        ddl: `CREATE TABLE orders (
  id INTEGER PRIMARY KEY,
  customer_id INTEGER NOT NULL,
  order_date DATE NOT NULL,
  status VARCHAR(20) DEFAULT 'pending',
  total DECIMAL(10, 2),
  FOREIGN KEY (customer_id) REFERENCES customers(id)
);`,
      },
      {
        name: 'products',
        ddl: `CREATE TABLE products (
  id INTEGER PRIMARY KEY,
  name VARCHAR(200) NOT NULL,
  category VARCHAR(50),
  price DECIMAL(10, 2) NOT NULL,
  stock_quantity INTEGER DEFAULT 0
);`,
      },
      {
        name: 'order_items',
        ddl: `CREATE TABLE order_items (
  id INTEGER PRIMARY KEY,
  order_id INTEGER NOT NULL,
  product_id INTEGER NOT NULL,
  quantity INTEGER NOT NULL,
  price DECIMAL(10, 2) NOT NULL,
  FOREIGN KEY (order_id) REFERENCES orders(id),
  FOREIGN KEY (product_id) REFERENCES products(id)
);`,
      },
    ],
    sampleQueries: [
      {
        name: 'Recent Orders',
        sql: `SELECT o.id, c.name, o.order_date, o.total
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.order_date > DATE('now', '-30 days')
ORDER BY o.order_date DESC;`,
        description: 'Join with date filter',
      },
      {
        name: 'Top Products by Revenue',
        sql: `SELECT p.name, SUM(oi.quantity * oi.price) as revenue
FROM order_items oi
JOIN products p ON oi.product_id = p.id
GROUP BY p.id, p.name
ORDER BY revenue DESC
LIMIT 10;`,
        description: 'Aggregation with limit',
      },
      {
        name: 'Customer Order History',
        sql: `SELECT
  c.name,
  COUNT(o.id) as order_count,
  SUM(o.total) as total_spent,
  MAX(o.order_date) as last_order_date
FROM customers c
LEFT JOIN orders o ON c.id = o.customer_id
GROUP BY c.id, c.name
HAVING COUNT(o.id) > 0
ORDER BY total_spent DESC;`,
        description: 'Complex aggregation with HAVING',
      },
    ],
  },
];

import type { Engine, EngineConfig, Schema } from './types';

export const ENGINES: EngineConfig[] = [
  { id: 'postgresql-15', name: 'PostgreSQL', version: '15' },
  { id: 'postgresql-16', name: 'PostgreSQL', version: '16' },
  { id: 'postgresql-17', name: 'PostgreSQL', version: '17' },
  { id: 'mysql-8.0', name: 'MySQL', version: '8.0' },
  { id: 'mysql-8.4', name: 'MySQL', version: '8.4' },
  { id: 'mariadb-11', name: 'MariaDB', version: '11' },
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
  {
    name: 'TPC-H (Benchmark)',
    tables: [
      {
        name: 'customer',
        ddl: `CREATE TABLE customer (
  c_custkey INTEGER PRIMARY KEY,
  c_name VARCHAR(25) NOT NULL,
  c_address VARCHAR(40),
  c_nationkey INTEGER,
  c_phone CHAR(15),
  c_acctbal DECIMAL(15,2),
  c_mktsegment CHAR(10),
  c_comment VARCHAR(117)
);`,
      },
      {
        name: 'orders',
        ddl: `CREATE TABLE orders (
  o_orderkey INTEGER PRIMARY KEY,
  o_custkey INTEGER NOT NULL,
  o_orderstatus CHAR(1),
  o_totalprice DECIMAL(15,2),
  o_orderdate DATE,
  o_orderpriority CHAR(15),
  o_clerk CHAR(15),
  o_shippriority INTEGER,
  o_comment VARCHAR(79),
  FOREIGN KEY (o_custkey) REFERENCES customer(c_custkey)
);`,
      },
      {
        name: 'lineitem',
        ddl: `CREATE TABLE lineitem (
  l_orderkey INTEGER NOT NULL,
  l_partkey INTEGER NOT NULL,
  l_suppkey INTEGER NOT NULL,
  l_linenumber INTEGER NOT NULL,
  l_quantity DECIMAL(15,2),
  l_extendedprice DECIMAL(15,2),
  l_discount DECIMAL(15,2),
  l_tax DECIMAL(15,2),
  l_returnflag CHAR(1),
  l_linestatus CHAR(1),
  l_shipdate DATE,
  l_commitdate DATE,
  l_receiptdate DATE,
  l_shipinstruct CHAR(25),
  l_shipmode CHAR(10),
  l_comment VARCHAR(44),
  PRIMARY KEY (l_orderkey, l_linenumber),
  FOREIGN KEY (l_orderkey) REFERENCES orders(o_orderkey)
);`,
      },
    ],
    sampleQueries: [
      {
        name: 'Revenue by Order Priority',
        sql: `SELECT o_orderpriority, COUNT(*) as order_count
FROM orders
WHERE o_orderdate >= DATE '1995-01-01'
  AND o_orderdate < DATE '1995-04-01'
GROUP BY o_orderpriority
ORDER BY o_orderpriority;`,
        description: 'TPC-H Query 4 (simplified)',
      },
      {
        name: 'Top Customers by Revenue',
        sql: `SELECT c.c_name, c.c_custkey,
       SUM(o.o_totalprice) as revenue
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
GROUP BY c.c_custkey, c.c_name
ORDER BY revenue DESC
LIMIT 10;`,
        description: 'Customer revenue analysis',
      },
      {
        name: 'Shipping Analysis',
        sql: `SELECT l_shipmode,
       SUM(l_quantity) as total_quantity,
       AVG(l_discount) as avg_discount
FROM lineitem
WHERE l_shipdate >= DATE '1995-01-01'
  AND l_shipdate < DATE '1996-01-01'
GROUP BY l_shipmode
ORDER BY l_shipmode;`,
        description: 'Analyze shipping modes and discounts',
      },
    ],
  },
  {
    name: 'Sakila (DVD Rental)',
    tables: [
      {
        name: 'film',
        ddl: `CREATE TABLE film (
  film_id INTEGER PRIMARY KEY,
  title VARCHAR(255) NOT NULL,
  description TEXT,
  release_year INTEGER,
  language_id INTEGER NOT NULL,
  rental_duration INTEGER DEFAULT 3,
  rental_rate DECIMAL(4,2) DEFAULT 4.99,
  length INTEGER,
  replacement_cost DECIMAL(5,2) DEFAULT 19.99,
  rating VARCHAR(10) DEFAULT 'G',
  special_features TEXT
);`,
      },
      {
        name: 'actor',
        ddl: `CREATE TABLE actor (
  actor_id INTEGER PRIMARY KEY,
  first_name VARCHAR(45) NOT NULL,
  last_name VARCHAR(45) NOT NULL,
  last_update TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);`,
      },
      {
        name: 'film_actor',
        ddl: `CREATE TABLE film_actor (
  actor_id INTEGER NOT NULL,
  film_id INTEGER NOT NULL,
  last_update TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (actor_id, film_id),
  FOREIGN KEY (actor_id) REFERENCES actor(actor_id),
  FOREIGN KEY (film_id) REFERENCES film(film_id)
);`,
      },
      {
        name: 'inventory',
        ddl: `CREATE TABLE inventory (
  inventory_id INTEGER PRIMARY KEY,
  film_id INTEGER NOT NULL,
  store_id INTEGER NOT NULL,
  last_update TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (film_id) REFERENCES film(film_id)
);`,
      },
      {
        name: 'rental',
        ddl: `CREATE TABLE rental (
  rental_id INTEGER PRIMARY KEY,
  rental_date TIMESTAMP NOT NULL,
  inventory_id INTEGER NOT NULL,
  customer_id INTEGER NOT NULL,
  return_date TIMESTAMP,
  staff_id INTEGER NOT NULL,
  last_update TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (inventory_id) REFERENCES inventory(inventory_id)
);`,
      },
    ],
    sampleQueries: [
      {
        name: 'Most Popular Films',
        sql: `SELECT f.title, COUNT(r.rental_id) as rental_count
FROM film f
JOIN inventory i ON f.film_id = i.film_id
JOIN rental r ON i.inventory_id = r.inventory_id
GROUP BY f.film_id, f.title
ORDER BY rental_count DESC
LIMIT 10;`,
        description: 'Top 10 most rented films',
      },
      {
        name: 'Actor Filmography',
        sql: `SELECT a.first_name, a.last_name,
       COUNT(fa.film_id) as film_count
FROM actor a
JOIN film_actor fa ON a.actor_id = fa.actor_id
GROUP BY a.actor_id, a.first_name, a.last_name
HAVING COUNT(fa.film_id) > 20
ORDER BY film_count DESC;`,
        description: 'Prolific actors with many films',
      },
      {
        name: 'Revenue by Film Rating',
        sql: `SELECT f.rating,
       COUNT(*) as film_count,
       AVG(f.rental_rate) as avg_rental_rate,
       SUM(f.rental_rate) as total_revenue
FROM film f
GROUP BY f.rating
ORDER BY total_revenue DESC;`,
        description: 'Analyze revenue by film rating',
      },
    ],
  },
  {
    name: 'Blog Platform',
    tables: [
      {
        name: 'users',
        ddl: `CREATE TABLE users (
  id INTEGER PRIMARY KEY,
  username VARCHAR(50) UNIQUE NOT NULL,
  email VARCHAR(100) UNIQUE NOT NULL,
  password_hash VARCHAR(255) NOT NULL,
  display_name VARCHAR(100),
  bio TEXT,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  last_login TIMESTAMP
);`,
      },
      {
        name: 'posts',
        ddl: `CREATE TABLE posts (
  id INTEGER PRIMARY KEY,
  author_id INTEGER NOT NULL,
  title VARCHAR(200) NOT NULL,
  slug VARCHAR(200) UNIQUE NOT NULL,
  content TEXT NOT NULL,
  excerpt TEXT,
  status VARCHAR(20) DEFAULT 'draft',
  published_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (author_id) REFERENCES users(id)
);`,
      },
      {
        name: 'comments',
        ddl: `CREATE TABLE comments (
  id INTEGER PRIMARY KEY,
  post_id INTEGER NOT NULL,
  author_id INTEGER,
  parent_id INTEGER,
  content TEXT NOT NULL,
  status VARCHAR(20) DEFAULT 'pending',
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (post_id) REFERENCES posts(id),
  FOREIGN KEY (author_id) REFERENCES users(id),
  FOREIGN KEY (parent_id) REFERENCES comments(id)
);`,
      },
      {
        name: 'tags',
        ddl: `CREATE TABLE tags (
  id INTEGER PRIMARY KEY,
  name VARCHAR(50) UNIQUE NOT NULL,
  slug VARCHAR(50) UNIQUE NOT NULL
);`,
      },
      {
        name: 'post_tags',
        ddl: `CREATE TABLE post_tags (
  post_id INTEGER NOT NULL,
  tag_id INTEGER NOT NULL,
  PRIMARY KEY (post_id, tag_id),
  FOREIGN KEY (post_id) REFERENCES posts(id),
  FOREIGN KEY (tag_id) REFERENCES tags(id)
);`,
      },
    ],
    sampleQueries: [
      {
        name: 'Recent Published Posts',
        sql: `SELECT p.title, u.display_name, p.published_at
FROM posts p
JOIN users u ON p.author_id = u.id
WHERE p.status = 'published'
  AND p.published_at IS NOT NULL
ORDER BY p.published_at DESC
LIMIT 10;`,
        description: 'Get latest published blog posts',
      },
      {
        name: 'Popular Tags',
        sql: `SELECT t.name, COUNT(pt.post_id) as post_count
FROM tags t
JOIN post_tags pt ON t.id = pt.tag_id
JOIN posts p ON pt.post_id = p.id
WHERE p.status = 'published'
GROUP BY t.id, t.name
ORDER BY post_count DESC
LIMIT 10;`,
        description: 'Most used tags on published posts',
      },
      {
        name: 'Comment Thread',
        sql: `SELECT c.id, c.parent_id, c.content,
       u.display_name as author,
       c.created_at
FROM comments c
LEFT JOIN users u ON c.author_id = u.id
WHERE c.post_id = 1
  AND c.status = 'approved'
ORDER BY c.created_at;`,
        description: 'Get comment thread for a post',
      },
      {
        name: 'Author Statistics',
        sql: `SELECT u.display_name,
       COUNT(p.id) as post_count,
       COUNT(DISTINCT c.id) as comment_count,
       MAX(p.published_at) as latest_post
FROM users u
LEFT JOIN posts p ON u.id = p.author_id AND p.status = 'published'
LEFT JOIN comments c ON u.id = c.author_id AND c.status = 'approved'
GROUP BY u.id, u.display_name
HAVING COUNT(p.id) > 0
ORDER BY post_count DESC;`,
        description: 'Author activity and engagement',
      },
    ],
  },
];

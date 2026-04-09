-- E-Commerce Schema Data Generation for MariaDB

-- Generate 10,000 customers
INSERT INTO customers (name, email, city, country, created_at)
WITH RECURSIVE nums AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM nums WHERE n < 10000
)
SELECT
  CONCAT('Customer ', n),
  CONCAT('customer', n, '@example.com'),
  ELT((n % 10) + 1, 'San Francisco', 'New York', 'Los Angeles', 'Chicago', 'Houston', 'Phoenix', 'Philadelphia', 'San Antonio', 'San Diego', 'Dallas'),
  ELT((n % 6) + 1, 'USA', 'Canada', 'UK', 'Germany', 'France', 'Australia'),
  DATE_SUB(NOW(), INTERVAL FLOOR(RAND(n) * 1825) DAY)
FROM nums;

-- Generate 1,000 products
INSERT INTO products (name, category, price, stock_quantity)
WITH RECURSIVE nums AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM nums WHERE n < 1000
)
SELECT
  CONCAT('Product ', n),
  ELT((n % 8) + 1, 'Electronics', 'Clothing', 'Books', 'Home & Garden', 'Sports', 'Toys', 'Food', 'Beauty'),
  5 + FLOOR(RAND(n) * 995),
  FLOOR(RAND(n * 2) * 1000)
FROM nums;

-- Generate 100,000 orders
INSERT INTO orders (customer_id, order_date, status, total)
WITH RECURSIVE nums AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM nums WHERE n < 100000
)
SELECT
  1 + FLOOR(RAND(n) * 9999),
  DATE_SUB(CURDATE(), INTERVAL FLOOR(RAND(n * 2) * 730) DAY),
  ELT(FLOOR(RAND(n * 3) * 5) + 1, 'pending', 'processing', 'shipped', 'delivered', 'cancelled'),
  10 + FLOOR(RAND(n * 4) * 1990)
FROM nums;

-- Generate 100,000 order_items (reduced from 500k for better compatibility)
INSERT INTO order_items (order_id, product_id, quantity, price)
WITH RECURSIVE nums AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM nums WHERE n < 100000
)
SELECT
  1 + FLOOR(RAND(n) * 99999),
  1 + FLOOR(RAND(n * 2) * 999),
  1 + FLOOR(RAND(n * 3) * 5),
  5 + FLOOR(RAND(n * 4) * 995)
FROM nums;

-- Create indexes
CREATE INDEX idx_orders_customer_id ON orders(customer_id);
CREATE INDEX idx_orders_date ON orders(order_date);
CREATE INDEX idx_orders_status ON orders(status);
CREATE INDEX idx_order_items_order_id ON order_items(order_id);
CREATE INDEX idx_order_items_product_id ON order_items(product_id);
CREATE INDEX idx_products_category ON products(category);
CREATE INDEX idx_customers_city ON customers(city);
CREATE INDEX idx_customers_country ON customers(country);

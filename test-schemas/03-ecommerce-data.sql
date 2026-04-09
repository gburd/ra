-- E-Commerce Schema Data Generation
-- Generates 100K customers, 1M orders, 10K products, and 5M order_items

\echo 'Generating E-Commerce test data...'

-- Generate 100,000 customers
INSERT INTO customers (name, email, city, country, created_at)
SELECT
  'Customer ' || i,
  'customer' || i || '@example.com',
  (ARRAY['San Francisco', 'New York', 'Los Angeles', 'Chicago', 'Houston', 'Phoenix', 'Philadelphia', 'San Antonio', 'San Diego', 'Dallas'])[1 + (i % 10)],
  (ARRAY['USA', 'Canada', 'UK', 'Germany', 'France', 'Australia'])[1 + (i % 6)],
  CURRENT_TIMESTAMP - (random() * 365 * 5 || ' days')::interval
FROM generate_series(1, 100000) AS i;

\echo 'Generated 100,000 customers'

-- Generate 10,000 products
INSERT INTO products (name, category, price, stock_quantity)
SELECT
  'Product ' || i,
  (ARRAY['Electronics', 'Clothing', 'Books', 'Home & Garden', 'Sports', 'Toys', 'Food', 'Beauty'])[1 + (i % 8)],
  (5 + random() * 995)::decimal(10,2),
  (random() * 1000)::int
FROM generate_series(1, 10000) AS i;

\echo 'Generated 10,000 products'

-- Generate 1,000,000 orders
INSERT INTO orders (customer_id, order_date, status, total)
SELECT
  1 + (random() * 99999)::int,
  CURRENT_DATE - (random() * 730)::int,
  (ARRAY['pending', 'processing', 'shipped', 'delivered', 'cancelled'])[1 + (random() * 4)::int],
  (10 + random() * 1990)::decimal(10,2)
FROM generate_series(1, 1000000) AS i;

\echo 'Generated 1,000,000 orders'

-- Generate 5,000,000 order_items (average 5 items per order)
INSERT INTO order_items (order_id, product_id, quantity, price)
SELECT
  1 + (random() * 999999)::int,
  1 + (random() * 9999)::int,
  1 + (random() * 5)::int,
  (5 + random() * 995)::decimal(10,2)
FROM generate_series(1, 5000000) AS i;

\echo 'Generated 5,000,000 order_items'

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_orders_customer_id ON orders(customer_id);
CREATE INDEX IF NOT EXISTS idx_orders_date ON orders(order_date);
CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status);
CREATE INDEX IF NOT EXISTS idx_order_items_order_id ON order_items(order_id);
CREATE INDEX IF NOT EXISTS idx_order_items_product_id ON order_items(product_id);
CREATE INDEX IF NOT EXISTS idx_products_category ON products(category);
CREATE INDEX IF NOT EXISTS idx_customers_city ON customers(city);
CREATE INDEX IF NOT EXISTS idx_customers_country ON customers(country);

\echo 'E-Commerce test data generation complete'

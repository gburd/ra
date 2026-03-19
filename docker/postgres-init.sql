-- RA Test Database - PostgreSQL Schema
-- Used for integration testing of metadata extraction and query optimization

BEGIN;

-- Customers table
CREATE TABLE customers (
    customer_id   SERIAL PRIMARY KEY,
    name          VARCHAR(100) NOT NULL,
    email         VARCHAR(255) NOT NULL UNIQUE,
    region        VARCHAR(50) NOT NULL DEFAULT 'US',
    credit_limit  NUMERIC(10, 2) NOT NULL DEFAULT 0.00
        CHECK (credit_limit >= 0),
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_customers_region ON customers (region);
CREATE INDEX idx_customers_active ON customers (active) WHERE active;

-- Orders table
CREATE TABLE orders (
    order_id     SERIAL PRIMARY KEY,
    customer_id  INTEGER NOT NULL
        REFERENCES customers (customer_id) ON DELETE CASCADE,
    order_date   DATE NOT NULL DEFAULT CURRENT_DATE,
    status       VARCHAR(20) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'shipped', 'delivered', 'cancelled')),
    total_amount NUMERIC(12, 2) NOT NULL
        CHECK (total_amount >= 0),
    notes        TEXT
);

CREATE INDEX idx_orders_customer ON orders (customer_id);
CREATE INDEX idx_orders_date ON orders (order_date);
CREATE INDEX idx_orders_status ON orders (status);

-- Order items (line items)
CREATE TABLE order_items (
    item_id    SERIAL PRIMARY KEY,
    order_id   INTEGER NOT NULL
        REFERENCES orders (order_id) ON DELETE CASCADE,
    product    VARCHAR(200) NOT NULL,
    quantity   INTEGER NOT NULL CHECK (quantity > 0),
    unit_price NUMERIC(10, 2) NOT NULL CHECK (unit_price >= 0)
);

CREATE INDEX idx_items_order ON order_items (order_id);

-- Products table for join testing
CREATE TABLE products (
    product_id  SERIAL PRIMARY KEY,
    name        VARCHAR(200) NOT NULL,
    category    VARCHAR(50) NOT NULL,
    price       NUMERIC(10, 2) NOT NULL CHECK (price >= 0),
    in_stock    BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX idx_products_category ON products (category);

-- Trigger: update order total when items change
CREATE OR REPLACE FUNCTION update_order_total()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE orders
    SET total_amount = (
        SELECT COALESCE(SUM(quantity * unit_price), 0)
        FROM order_items
        WHERE order_id = COALESCE(NEW.order_id, OLD.order_id)
    )
    WHERE order_id = COALESCE(NEW.order_id, OLD.order_id);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_order_total
    AFTER INSERT OR UPDATE OR DELETE ON order_items
    FOR EACH ROW EXECUTE FUNCTION update_order_total();

-- View for testing view metadata extraction
CREATE VIEW customer_order_summary AS
SELECT
    c.customer_id,
    c.name,
    c.region,
    COUNT(o.order_id)        AS order_count,
    COALESCE(SUM(o.total_amount), 0) AS lifetime_value
FROM customers c
LEFT JOIN orders o ON c.customer_id = o.customer_id
GROUP BY c.customer_id, c.name, c.region;

-- Sample data
INSERT INTO customers (name, email, region, credit_limit, active) VALUES
    ('Alice Johnson',  'alice@example.com',  'US',     5000.00, TRUE),
    ('Bob Smith',      'bob@example.com',    'EU',     3000.00, TRUE),
    ('Carol Williams', 'carol@example.com',  'US',     7500.00, TRUE),
    ('Dave Brown',     'dave@example.com',   'APAC',   2000.00, FALSE),
    ('Eve Davis',      'eve@example.com',    'EU',    10000.00, TRUE);

INSERT INTO products (name, category, price, in_stock) VALUES
    ('Widget A',    'hardware',  29.99, TRUE),
    ('Widget B',    'hardware',  49.99, TRUE),
    ('Service X',   'software', 199.99, TRUE),
    ('Service Y',   'software',  99.99, TRUE),
    ('Adapter Z',   'hardware',   9.99, FALSE);

INSERT INTO orders (customer_id, order_date, status, total_amount) VALUES
    (1, '2024-01-15', 'delivered',  0),
    (1, '2024-03-20', 'shipped',    0),
    (2, '2024-02-10', 'delivered',  0),
    (3, '2024-03-01', 'pending',    0),
    (5, '2024-01-05', 'cancelled',  0);

INSERT INTO order_items (order_id, product, quantity, unit_price) VALUES
    (1, 'Widget A',  2, 29.99),
    (1, 'Service X', 1, 199.99),
    (2, 'Widget B',  3, 49.99),
    (3, 'Service Y', 1, 99.99),
    (4, 'Widget A',  5, 29.99),
    (4, 'Adapter Z', 10, 9.99),
    (5, 'Service X', 1, 199.99);

-- Refresh statistics for the query planner
ANALYZE customers;
ANALYZE orders;
ANALYZE order_items;
ANALYZE products;

COMMIT;

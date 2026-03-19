-- RA Test Database - MySQL Schema
-- Used for integration testing of metadata extraction and query optimization

-- Customers table
CREATE TABLE customers (
    customer_id   INT AUTO_INCREMENT PRIMARY KEY,
    name          VARCHAR(100) NOT NULL,
    email         VARCHAR(255) NOT NULL UNIQUE,
    region        VARCHAR(50) NOT NULL DEFAULT 'US',
    credit_limit  DECIMAL(10, 2) NOT NULL DEFAULT 0.00
        CHECK (credit_limit >= 0),
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_customers_region ON customers (region);
CREATE INDEX idx_customers_active ON customers (active);

-- Orders table
CREATE TABLE orders (
    order_id     INT AUTO_INCREMENT PRIMARY KEY,
    customer_id  INT NOT NULL,
    order_date   DATE NOT NULL DEFAULT (CURRENT_DATE),
    status       VARCHAR(20) NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'shipped', 'delivered', 'cancelled')),
    total_amount DECIMAL(12, 2) NOT NULL DEFAULT 0.00
        CHECK (total_amount >= 0),
    notes        TEXT,
    FOREIGN KEY (customer_id) REFERENCES customers (customer_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_orders_customer ON orders (customer_id);
CREATE INDEX idx_orders_date ON orders (order_date);
CREATE INDEX idx_orders_status ON orders (status);

-- Order items (line items)
CREATE TABLE order_items (
    item_id    INT AUTO_INCREMENT PRIMARY KEY,
    order_id   INT NOT NULL,
    product    VARCHAR(200) NOT NULL,
    quantity   INT NOT NULL CHECK (quantity > 0),
    unit_price DECIMAL(10, 2) NOT NULL CHECK (unit_price >= 0),
    FOREIGN KEY (order_id) REFERENCES orders (order_id)
        ON DELETE CASCADE
);

CREATE INDEX idx_items_order ON order_items (order_id);

-- Products table for join testing
CREATE TABLE products (
    product_id  INT AUTO_INCREMENT PRIMARY KEY,
    name        VARCHAR(200) NOT NULL,
    category    VARCHAR(50) NOT NULL,
    price       DECIMAL(10, 2) NOT NULL CHECK (price >= 0),
    in_stock    BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX idx_products_category ON products (category);

-- Trigger: update order total when items are inserted
DELIMITER //
CREATE TRIGGER trg_order_total_insert
    AFTER INSERT ON order_items
    FOR EACH ROW
BEGIN
    UPDATE orders
    SET total_amount = (
        SELECT COALESCE(SUM(quantity * unit_price), 0)
        FROM order_items
        WHERE order_id = NEW.order_id
    )
    WHERE order_id = NEW.order_id;
END//

CREATE TRIGGER trg_order_total_update
    AFTER UPDATE ON order_items
    FOR EACH ROW
BEGIN
    UPDATE orders
    SET total_amount = (
        SELECT COALESCE(SUM(quantity * unit_price), 0)
        FROM order_items
        WHERE order_id = NEW.order_id
    )
    WHERE order_id = NEW.order_id;
END//

CREATE TRIGGER trg_order_total_delete
    AFTER DELETE ON order_items
    FOR EACH ROW
BEGIN
    UPDATE orders
    SET total_amount = (
        SELECT COALESCE(SUM(quantity * unit_price), 0)
        FROM order_items
        WHERE order_id = OLD.order_id
    )
    WHERE order_id = OLD.order_id;
END//
DELIMITER ;

-- View for testing view metadata extraction
CREATE VIEW customer_order_summary AS
SELECT
    c.customer_id,
    c.name,
    c.region,
    COUNT(o.order_id)                  AS order_count,
    COALESCE(SUM(o.total_amount), 0)   AS lifetime_value
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

-- Refresh statistics for the query optimizer
ANALYZE TABLE customers;
ANALYZE TABLE orders;
ANALYZE TABLE order_items;
ANALYZE TABLE products;

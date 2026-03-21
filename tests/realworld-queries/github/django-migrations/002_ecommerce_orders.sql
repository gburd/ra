-- E-commerce Order System (Django e-commerce apps)
-- Source: Oscar, Django-Shop, Saleor
-- Pattern: OLTP with complex joins and aggregations

CREATE TABLE orders (
    id BIGINT PRIMARY KEY,
    user_id INTEGER NOT NULL,
    status VARCHAR(20) NOT NULL,
    total_amount DECIMAL(10, 2) NOT NULL,
    currency VARCHAR(3) NOT NULL DEFAULT 'USD',
    shipping_address_id INTEGER,
    billing_address_id INTEGER,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TIMESTAMP NULL
);

CREATE TABLE order_items (
    id BIGINT PRIMARY KEY,
    order_id BIGINT NOT NULL,
    product_id INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    unit_price DECIMAL(10, 2) NOT NULL,
    discount DECIMAL(10, 2) DEFAULT 0.00,
    FOREIGN KEY (order_id) REFERENCES orders(id)
);

CREATE TABLE products (
    id INTEGER PRIMARY KEY,
    sku VARCHAR(50) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    price DECIMAL(10, 2) NOT NULL,
    stock_quantity INTEGER NOT NULL DEFAULT 0,
    category_id INTEGER
);

CREATE INDEX idx_orders_user_id ON orders(user_id);
CREATE INDEX idx_orders_status ON orders(status);
CREATE INDEX idx_orders_created_at ON orders(created_at);
CREATE INDEX idx_order_items_order_id ON order_items(order_id);
CREATE INDEX idx_order_items_product_id ON order_items(product_id);

-- Real-world query: Order details with items
SELECT
    o.id AS order_id,
    o.status,
    o.total_amount,
    o.created_at,
    oi.id AS item_id,
    oi.quantity,
    oi.unit_price,
    p.name AS product_name,
    p.sku
FROM orders o
JOIN order_items oi ON o.id = oi.order_id
JOIN products p ON oi.product_id = p.id
WHERE o.user_id = 12345 AND o.status IN ('pending', 'processing', 'shipped')
ORDER BY o.created_at DESC;

-- Analytics: Top selling products in last 30 days
SELECT
    p.id,
    p.name,
    p.sku,
    SUM(oi.quantity) AS total_sold,
    SUM(oi.quantity * oi.unit_price) AS revenue
FROM order_items oi
JOIN products p ON oi.product_id = p.id
JOIN orders o ON oi.order_id = o.id
WHERE o.created_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
    AND o.status = 'completed'
GROUP BY p.id, p.name, p.sku
ORDER BY revenue DESC
LIMIT 100;

-- Low stock alert
SELECT
    p.id,
    p.sku,
    p.name,
    p.stock_quantity,
    COALESCE(SUM(oi.quantity), 0) AS pending_quantity
FROM products p
LEFT JOIN order_items oi ON p.id = oi.product_id
LEFT JOIN orders o ON oi.order_id = o.id
    AND o.status IN ('pending', 'processing')
WHERE p.stock_quantity < 10
GROUP BY p.id, p.sku, p.name, p.stock_quantity
HAVING p.stock_quantity - COALESCE(SUM(oi.quantity), 0) < 5
ORDER BY p.stock_quantity ASC;

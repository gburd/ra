-- E-Commerce Schema - MySQL Version
-- Demonstrates complex JOINs, aggregations, window functions, and date filtering

-- Create customers table
CREATE TABLE customers (
    customer_id INT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(100) UNIQUE NOT NULL,
    country VARCHAR(50) NOT NULL,
    city VARCHAR(100) NOT NULL,
    signup_date DATE NOT NULL,
    customer_tier VARCHAR(20) DEFAULT 'standard',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Create products table
CREATE TABLE products (
    product_id INT AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(200) NOT NULL,
    category VARCHAR(50) NOT NULL,
    price DECIMAL(10,2) NOT NULL,
    stock_quantity INT NOT NULL,
    supplier VARCHAR(100),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Create orders table
CREATE TABLE orders (
    order_id INT AUTO_INCREMENT PRIMARY KEY,
    customer_id INT,
    order_date DATE NOT NULL,
    total_amount DECIMAL(12,2) NOT NULL,
    status VARCHAR(20) NOT NULL,
    shipping_country VARCHAR(50) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (customer_id) REFERENCES customers(customer_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Create order_items table
CREATE TABLE order_items (
    order_item_id INT AUTO_INCREMENT PRIMARY KEY,
    order_id INT,
    product_id INT,
    quantity INT NOT NULL,
    unit_price DECIMAL(10,2) NOT NULL,
    discount_percent DECIMAL(5,2) DEFAULT 0.00,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (order_id) REFERENCES orders(order_id),
    FOREIGN KEY (product_id) REFERENCES products(product_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Create indexes
CREATE INDEX idx_customers_country ON customers(country);
CREATE INDEX idx_customers_tier ON customers(customer_tier);
CREATE INDEX idx_customers_signup ON customers(signup_date);
CREATE INDEX idx_products_category ON products(category);
CREATE INDEX idx_products_price ON products(price);
CREATE INDEX idx_orders_customer ON orders(customer_id);
CREATE INDEX idx_orders_date ON orders(order_date);
CREATE INDEX idx_orders_status ON orders(status);
CREATE INDEX idx_order_items_order ON order_items(order_id);
CREATE INDEX idx_order_items_product ON order_items(product_id);

-- Insert sample customers (50 customers)
INSERT INTO customers (name, email, country, city, signup_date, customer_tier) VALUES
    ('John Doe', 'john.doe@email.com', 'USA', 'New York', '2022-01-15', 'gold'),
    ('Jane Smith', 'jane.smith@email.com', 'USA', 'Los Angeles', '2022-02-20', 'platinum'),
    ('Bob Johnson', 'bob.johnson@email.com', 'UK', 'London', '2022-03-10', 'standard'),
    ('Alice Brown', 'alice.brown@email.com', 'Canada', 'Toronto', '2022-03-25', 'gold'),
    ('Charlie Wilson', 'charlie.wilson@email.com', 'USA', 'Chicago', '2022-04-05', 'standard'),
    ('Diana Davis', 'diana.davis@email.com', 'USA', 'Houston', '2022-04-20', 'silver'),
    ('Edward Miller', 'edward.miller@email.com', 'UK', 'Manchester', '2022-05-01', 'gold'),
    ('Fiona Garcia', 'fiona.garcia@email.com', 'Spain', 'Madrid', '2022-05-15', 'standard'),
    ('George Martinez', 'george.martinez@email.com', 'USA', 'Phoenix', '2022-06-01', 'silver'),
    ('Hannah Rodriguez', 'hannah.rodriguez@email.com', 'Mexico', 'Mexico City', '2022-06-15', 'platinum'),
    ('Ian Lopez', 'ian.lopez@email.com', 'USA', 'Philadelphia', '2022-07-01', 'standard'),
    ('Julia Hernandez', 'julia.hernandez@email.com', 'USA', 'San Antonio', '2022-07-15', 'gold'),
    ('Kevin Gonzalez', 'kevin.gonzalez@email.com', 'USA', 'San Diego', '2022-08-01', 'silver'),
    ('Laura Perez', 'laura.perez@email.com', 'Argentina', 'Buenos Aires', '2022-08-15', 'standard'),
    ('Michael Torres', 'michael.torres@email.com', 'USA', 'Dallas', '2022-09-01', 'platinum'),
    ('Nancy Rivera', 'nancy.rivera@email.com', 'USA', 'San Jose', '2022-09-15', 'gold'),
    ('Oscar Flores', 'oscar.flores@email.com', 'Colombia', 'Bogota', '2022-10-01', 'standard'),
    ('Patricia Gomez', 'patricia.gomez@email.com', 'USA', 'Austin', '2022-10-15', 'silver'),
    ('Quentin Diaz', 'quentin.diaz@email.com', 'USA', 'Jacksonville', '2022-11-01', 'gold'),
    ('Rachel Reyes', 'rachel.reyes@email.com', 'USA', 'Fort Worth', '2022-11-15', 'standard'),
    ('Steven Cruz', 'steven.cruz@email.com', 'Brazil', 'São Paulo', '2022-12-01', 'platinum'),
    ('Teresa Morales', 'teresa.morales@email.com', 'USA', 'Columbus', '2022-12-15', 'silver'),
    ('Ulysses Ortiz', 'ulysses.ortiz@email.com', 'Chile', 'Santiago', '2023-01-01', 'standard'),
    ('Victoria Gutierrez', 'victoria.gutierrez@email.com', 'USA', 'Charlotte', '2023-01-15', 'gold'),
    ('William Jimenez', 'william.jimenez@email.com', 'USA', 'San Francisco', '2023-02-01', 'platinum'),
    ('Xena Ruiz', 'xena.ruiz@email.com', 'USA', 'Indianapolis', '2023-02-15', 'standard'),
    ('Yolanda Hernandez', 'yolanda.hernandez@email.com', 'USA', 'Seattle', '2023-03-01', 'silver'),
    ('Zachary Medina', 'zachary.medina@email.com', 'Canada', 'Vancouver', '2023-03-15', 'gold'),
    ('Amanda Aguilar', 'amanda.aguilar@email.com', 'USA', 'Denver', '2023-04-01', 'standard'),
    ('Brandon Vega', 'brandon.vega@email.com', 'USA', 'Washington', '2023-04-15', 'platinum'),
    ('Christina Castro', 'christina.castro@email.com', 'USA', 'Boston', '2023-05-01', 'silver'),
    ('Daniel Mendoza', 'daniel.mendoza@email.com', 'Peru', 'Lima', '2023-05-15', 'standard'),
    ('Emma Ramos', 'emma.ramos@email.com', 'USA', 'Nashville', '2023-06-01', 'gold'),
    ('Frank Vargas', 'frank.vargas@email.com', 'USA', 'Detroit', '2023-06-15', 'silver'),
    ('Grace Romero', 'grace.romero@email.com', 'USA', 'Oklahoma City', '2023-07-01', 'standard'),
    ('Henry Soto', 'henry.soto@email.com', 'USA', 'Portland', '2023-07-15', 'platinum'),
    ('Isabel Contreras', 'isabel.contreras@email.com', 'USA', 'Las Vegas', '2023-08-01', 'gold'),
    ('Jack Vazquez', 'jack.vazquez@email.com', 'USA', 'Louisville', '2023-08-15', 'standard'),
    ('Katherine Castillo', 'katherine.castillo@email.com', 'USA', 'Milwaukee', '2023-09-01', 'silver'),
    ('Leo Mendez', 'leo.mendez@email.com', 'USA', 'Albuquerque', '2023-09-15', 'gold'),
    ('Maria Silva', 'maria.silva@email.com', 'Portugal', 'Lisbon', '2023-10-01', 'platinum'),
    ('Nathan Rojas', 'nathan.rojas@email.com', 'USA', 'Tucson', '2023-10-15', 'standard'),
    ('Olivia Marquez', 'olivia.marquez@email.com', 'USA', 'Fresno', '2023-11-01', 'silver'),
    ('Peter Campos', 'peter.campos@email.com', 'USA', 'Sacramento', '2023-11-15', 'gold'),
    ('Quinn Acosta', 'quinn.acosta@email.com', 'USA', 'Mesa', '2023-12-01', 'standard'),
    ('Rita Delgado', 'rita.delgado@email.com', 'USA', 'Kansas City', '2023-12-15', 'platinum'),
    ('Samuel Pacheco', 'samuel.pacheco@email.com', 'USA', 'Atlanta', '2024-01-01', 'silver'),
    ('Tara Cervantes', 'tara.cervantes@email.com', 'USA', 'Miami', '2024-01-15', 'gold'),
    ('Ursula Sandoval', 'ursula.sandoval@email.com', 'USA', 'Long Beach', '2024-02-01', 'standard'),
    ('Victor Fuentes', 'victor.fuentes@email.com', 'USA', 'Colorado Springs', '2024-02-15', 'silver');

-- Insert sample products (30 products across multiple categories)
INSERT INTO products (name, category, price, stock_quantity, supplier) VALUES
    ('Laptop Pro 15', 'Electronics', 1299.99, 50, 'TechSupplier Inc'),
    ('Wireless Mouse', 'Electronics', 29.99, 200, 'TechSupplier Inc'),
    ('USB-C Cable', 'Electronics', 12.99, 500, 'TechSupplier Inc'),
    ('Monitor 27"', 'Electronics', 399.99, 75, 'DisplayCo'),
    ('Mechanical Keyboard', 'Electronics', 89.99, 150, 'TechSupplier Inc'),
    ('Office Chair Deluxe', 'Furniture', 249.99, 30, 'FurniturePlus'),
    ('Standing Desk', 'Furniture', 449.99, 20, 'FurniturePlus'),
    ('Bookshelf', 'Furniture', 129.99, 40, 'FurniturePlus'),
    ('Desk Lamp LED', 'Furniture', 39.99, 100, 'LightingWorld'),
    ('Filing Cabinet', 'Furniture', 179.99, 25, 'FurniturePlus'),
    ('Running Shoes', 'Sports', 89.99, 120, 'SportGear Ltd'),
    ('Yoga Mat', 'Sports', 24.99, 200, 'SportGear Ltd'),
    ('Dumbbells Set', 'Sports', 69.99, 80, 'FitnessSupply'),
    ('Water Bottle', 'Sports', 14.99, 300, 'SportGear Ltd'),
    ('Backpack Large', 'Sports', 54.99, 150, 'SportGear Ltd'),
    ('Novel - Fiction', 'Books', 16.99, 100, 'BookDistributors'),
    ('Programming Guide', 'Books', 49.99, 60, 'BookDistributors'),
    ('Cookbook', 'Books', 29.99, 80, 'BookDistributors'),
    ('Biography', 'Books', 24.99, 70, 'BookDistributors'),
    ('Dictionary', 'Books', 34.99, 50, 'BookDistributors'),
    ('Coffee Maker', 'Home', 79.99, 90, 'HomeGoods Co'),
    ('Blender', 'Home', 59.99, 110, 'HomeGoods Co'),
    ('Vacuum Cleaner', 'Home', 199.99, 45, 'HomeGoods Co'),
    ('Bedding Set', 'Home', 89.99, 65, 'HomeGoods Co'),
    ('Towel Set', 'Home', 34.99, 150, 'HomeGoods Co'),
    ('Smartphone Pro', 'Electronics', 899.99, 100, 'MobileTech'),
    ('Tablet 10"', 'Electronics', 449.99, 80, 'MobileTech'),
    ('Headphones Wireless', 'Electronics', 149.99, 180, 'AudioPro'),
    ('Smart Watch', 'Electronics', 299.99, 90, 'MobileTech'),
    ('Camera DSLR', 'Electronics', 1099.99, 35, 'PhotoSupply');

-- Insert sample orders (55 orders with realistic distribution)
INSERT INTO orders (customer_id, order_date, total_amount, status, shipping_country) VALUES
    (1, '2023-01-15', 1329.98, 'delivered', 'USA'),
    (1, '2023-03-20', 89.99, 'delivered', 'USA'),
    (2, '2023-01-20', 449.99, 'delivered', 'USA'),
    (2, '2023-04-15', 1099.99, 'delivered', 'USA'),
    (3, '2023-02-01', 129.99, 'delivered', 'UK'),
    (4, '2023-02-10', 249.99, 'delivered', 'Canada'),
    (5, '2023-02-15', 89.99, 'delivered', 'USA'),
    (6, '2023-02-20', 79.99, 'delivered', 'USA'),
    (7, '2023-03-01', 399.99, 'delivered', 'UK'),
    (8, '2023-03-05', 54.99, 'delivered', 'Spain'),
    (9, '2023-03-10', 149.99, 'delivered', 'USA'),
    (10, '2023-03-15', 899.99, 'delivered', 'Mexico'),
    (11, '2023-04-01', 29.99, 'delivered', 'USA'),
    (12, '2023-04-05', 249.99, 'delivered', 'USA'),
    (13, '2023-04-10', 89.99, 'delivered', 'USA'),
    (14, '2023-04-15', 129.99, 'delivered', 'Argentina'),
    (15, '2023-05-01', 1299.99, 'delivered', 'USA'),
    (16, '2023-05-05', 449.99, 'delivered', 'USA'),
    (17, '2023-05-10', 79.99, 'delivered', 'Colombia'),
    (18, '2023-05-15', 199.99, 'delivered', 'USA'),
    (19, '2023-06-01', 299.99, 'delivered', 'USA'),
    (20, '2023-06-05', 54.99, 'delivered', 'USA'),
    (21, '2023-06-10', 1099.99, 'delivered', 'Brazil'),
    (22, '2023-06-15', 89.99, 'delivered', 'USA'),
    (23, '2023-07-01', 129.99, 'delivered', 'Chile'),
    (24, '2023-07-05', 449.99, 'delivered', 'USA'),
    (25, '2023-07-10', 899.99, 'delivered', 'USA'),
    (26, '2023-07-15', 149.99, 'delivered', 'USA'),
    (27, '2023-08-01', 249.99, 'delivered', 'USA'),
    (28, '2023-08-05', 399.99, 'delivered', 'Canada'),
    (29, '2023-08-10', 79.99, 'delivered', 'USA'),
    (30, '2023-08-15', 1299.99, 'delivered', 'USA'),
    (31, '2023-09-01', 199.99, 'delivered', 'USA'),
    (32, '2023-09-05', 89.99, 'delivered', 'Peru'),
    (33, '2023-09-10', 299.99, 'delivered', 'USA'),
    (34, '2023-09-15', 129.99, 'delivered', 'USA'),
    (35, '2023-10-01', 54.99, 'delivered', 'USA'),
    (36, '2023-10-05', 1099.99, 'delivered', 'USA'),
    (37, '2023-10-10', 449.99, 'delivered', 'USA'),
    (38, '2023-10-15', 89.99, 'delivered', 'USA'),
    (39, '2023-11-01', 249.99, 'delivered', 'USA'),
    (40, '2023-11-05', 899.99, 'delivered', 'USA'),
    (41, '2023-11-10', 1299.99, 'delivered', 'Portugal'),
    (42, '2023-11-15', 79.99, 'delivered', 'USA'),
    (43, '2023-12-01', 199.99, 'delivered', 'USA'),
    (44, '2023-12-05', 399.99, 'delivered', 'USA'),
    (45, '2023-12-10', 149.99, 'delivered', 'USA'),
    (46, '2023-12-15', 1099.99, 'delivered', 'USA'),
    (47, '2024-01-01', 89.99, 'delivered', 'USA'),
    (48, '2024-01-05', 299.99, 'delivered', 'USA'),
    (49, '2024-01-10', 449.99, 'delivered', 'USA'),
    (50, '2024-01-15', 129.99, 'shipped', 'USA'),
    (1, '2024-02-01', 899.99, 'shipped', 'USA'),
    (2, '2024-02-05', 249.99, 'processing', 'USA'),
    (3, '2024-02-10', 399.99, 'processing', 'UK');

-- Insert order items (linking orders to products with quantities)
-- Order 1: Laptop + Mouse
INSERT INTO order_items (order_id, product_id, quantity, unit_price, discount_percent) VALUES
    (1, 1, 1, 1299.99, 0.00),
    (1, 2, 1, 29.99, 0.00);

-- Order 2: Running Shoes
INSERT INTO order_items (order_id, product_id, quantity, unit_price, discount_percent) VALUES
    (2, 11, 1, 89.99, 0.00);

-- Continue pattern for remaining orders
INSERT INTO order_items (order_id, product_id, quantity, unit_price, discount_percent) VALUES
    (3, 7, 1, 449.99, 0.00),
    (4, 30, 1, 1099.99, 0.00),
    (5, 8, 1, 129.99, 0.00),
    (6, 6, 1, 249.99, 0.00),
    (7, 11, 1, 89.99, 0.00),
    (8, 21, 1, 79.99, 0.00),
    (9, 4, 1, 399.99, 0.00),
    (10, 15, 1, 54.99, 0.00),
    (11, 28, 1, 149.99, 0.00),
    (12, 26, 1, 899.99, 0.00),
    (13, 2, 1, 29.99, 0.00),
    (14, 6, 1, 249.99, 0.00),
    (15, 11, 1, 89.99, 0.00),
    (16, 8, 1, 129.99, 0.00),
    (17, 1, 1, 1299.99, 0.00),
    (18, 7, 1, 449.99, 0.00),
    (19, 21, 1, 79.99, 0.00),
    (20, 23, 1, 199.99, 0.00),
    (21, 29, 1, 299.99, 0.00),
    (22, 15, 1, 54.99, 0.00),
    (23, 30, 1, 1099.99, 0.00),
    (24, 11, 1, 89.99, 0.00),
    (25, 8, 1, 129.99, 0.00),
    (26, 7, 1, 449.99, 0.00),
    (27, 26, 1, 899.99, 0.00),
    (28, 28, 1, 149.99, 0.00),
    (29, 6, 1, 249.99, 0.00),
    (30, 4, 1, 399.99, 0.00),
    (31, 21, 1, 79.99, 0.00),
    (32, 1, 1, 1299.99, 0.00),
    (33, 23, 1, 199.99, 0.00),
    (34, 11, 1, 89.99, 0.00),
    (35, 29, 1, 299.99, 0.00),
    (36, 8, 1, 129.99, 0.00),
    (37, 15, 1, 54.99, 0.00),
    (38, 30, 1, 1099.99, 0.00),
    (39, 7, 1, 449.99, 0.00),
    (40, 11, 1, 89.99, 0.00),
    (41, 6, 1, 249.99, 0.00),
    (42, 26, 1, 899.99, 0.00),
    (43, 1, 1, 1299.99, 0.00),
    (44, 21, 1, 79.99, 0.00),
    (45, 23, 1, 199.99, 0.00),
    (46, 4, 1, 399.99, 0.00),
    (47, 28, 1, 149.99, 0.00),
    (48, 30, 1, 1099.99, 0.00),
    (49, 11, 1, 89.99, 0.00),
    (50, 29, 1, 299.99, 0.00),
    (51, 7, 1, 449.99, 0.00),
    (52, 8, 1, 129.99, 0.00),
    (53, 26, 1, 899.99, 0.00),
    (54, 6, 1, 249.99, 0.00),
    (55, 4, 1, 399.99, 0.00);

-- Analyze tables for optimal query planning
ANALYZE TABLE customers;
ANALYZE TABLE products;
ANALYZE TABLE orders;
ANALYZE TABLE order_items;

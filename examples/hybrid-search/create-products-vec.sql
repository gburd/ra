-- Create Product Vector database with sqlite-vec
-- Run: sqlite3 products-vec.db < create-products-vec.sql
-- Note: Requires sqlite-vec extension to be loaded

-- Create products table with embeddings
CREATE TABLE IF NOT EXISTS products (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    price REAL NOT NULL,
    embedding BLOB,  -- 384-dimensional vector (simulating sentence-transformers)
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- Insert sample products with mock embeddings
-- In real usage, embeddings would come from a model like sentence-transformers
INSERT INTO products (id, name, description, category, price) VALUES
(1, 'Wireless Bluetooth Headphones', 'Premium over-ear headphones with active noise cancellation and 30-hour battery life', 'Electronics', 149.99),
(2, 'USB-C Hub Adapter', '7-in-1 USB-C hub with HDMI, SD card reader, and power delivery', 'Electronics', 39.99),
(3, 'Mechanical Keyboard', 'RGB mechanical keyboard with Cherry MX switches and aluminum frame', 'Electronics', 129.99),
(4, 'Ergonomic Office Chair', 'Adjustable mesh office chair with lumbar support and armrests', 'Furniture', 299.99),
(5, 'Standing Desk', 'Electric height-adjustable standing desk with memory presets', 'Furniture', 499.99),
(6, 'Monitor Arm Mount', 'Gas spring monitor arm with cable management for displays up to 27 inches', 'Furniture', 79.99),
(7, 'Laptop Stand', 'Aluminum laptop stand with adjustable height and ventilation', 'Electronics', 44.99),
(8, 'Webcam 1080p', 'Full HD webcam with autofocus and built-in microphone', 'Electronics', 69.99),
(9, 'Desk Lamp LED', 'Adjustable LED desk lamp with touch controls and USB charging port', 'Furniture', 34.99),
(10, 'Cable Management Box', 'Large cable organizer box to hide power strips and cables', 'Accessories', 19.99),
(11, 'Wireless Mouse', 'Ergonomic wireless mouse with precision tracking and long battery life', 'Electronics', 29.99),
(12, 'Mouse Pad Extended', 'Extra large mouse pad with stitched edges and non-slip base', 'Accessories', 14.99),
(13, 'Monitor 27-inch 4K', '4K IPS monitor with HDR support and slim bezels', 'Electronics', 399.99),
(14, 'Document Scanner', 'Portable document scanner with automatic feeding and duplex scanning', 'Electronics', 199.99),
(15, 'Bookshelf', 'Modern 5-shelf bookcase with open design', 'Furniture', 159.99),
(16, 'Desk Organizer', 'Wooden desk organizer with compartments for office supplies', 'Accessories', 24.99),
(17, 'Wireless Charger', 'Fast wireless charging pad compatible with Qi-enabled devices', 'Electronics', 19.99),
(18, 'Portable SSD 1TB', 'Compact external SSD with USB 3.2 and shock resistance', 'Electronics', 119.99),
(19, 'Noise Machine', 'White noise machine with multiple sound options for better sleep', 'Wellness', 49.99),
(20, 'Air Purifier', 'HEPA air purifier for medium-sized rooms with quiet operation', 'Wellness', 179.99),
(21, 'Water Bottle', 'Insulated stainless steel water bottle that keeps drinks cold for 24 hours', 'Wellness', 27.99),
(22, 'Yoga Mat', 'Premium non-slip yoga mat with carrying strap', 'Wellness', 39.99),
(23, 'Resistance Bands', 'Set of 5 resistance bands with different strength levels', 'Wellness', 22.99),
(24, 'Foam Roller', 'High-density foam roller for muscle recovery and massage', 'Wellness', 29.99),
(25, 'Smart Watch', 'Fitness smartwatch with heart rate monitor and GPS', 'Electronics', 249.99),
(26, 'Bluetooth Speaker', 'Portable waterproof Bluetooth speaker with 360-degree sound', 'Electronics', 79.99),
(27, 'Reading Light', 'Clip-on LED reading light with adjustable brightness', 'Accessories', 16.99),
(28, 'Tablet Stand', 'Adjustable tablet holder for hands-free viewing', 'Accessories', 21.99),
(29, 'Phone Holder Car', 'Magnetic car phone mount with secure grip', 'Accessories', 18.99),
(30, 'Power Bank', '20000mAh portable charger with fast charging and dual USB ports', 'Electronics', 44.99),
(31, 'Laptop Sleeve', 'Padded laptop sleeve with extra pockets for accessories', 'Accessories', 24.99),
(32, 'Backpack Laptop', 'Business laptop backpack with USB charging port and anti-theft pocket', 'Accessories', 54.99),
(33, 'Coffee Maker', 'Programmable coffee maker with thermal carafe and brew strength control', 'Kitchen', 89.99),
(34, 'Electric Kettle', 'Stainless steel electric kettle with temperature control', 'Kitchen', 49.99),
(35, 'Blender', 'High-speed blender for smoothies and food processing', 'Kitchen', 79.99),
(36, 'Food Scale', 'Digital kitchen scale with precise measurements up to 11 lbs', 'Kitchen', 19.99),
(37, 'Knife Set', 'Professional chef knife set with wooden block', 'Kitchen', 129.99),
(38, 'Cutting Board Set', 'Bamboo cutting board set with juice grooves', 'Kitchen', 34.99),
(39, 'Storage Containers', 'Glass food storage containers with airtight lids - set of 10', 'Kitchen', 39.99),
(40, 'Dish Drying Rack', 'Large capacity dish rack with drainboard and utensil holder', 'Kitchen', 29.99),
(41, 'Trash Can', 'Touchless automatic trash can with motion sensor', 'Kitchen', 89.99),
(42, 'Paper Towel Holder', 'Wall-mounted paper towel holder with adhesive backing', 'Kitchen', 12.99),
(43, 'Spice Rack', 'Countertop spice rack with 20 glass jars and labels', 'Kitchen', 44.99),
(44, 'Pot and Pan Set', 'Non-stick cookware set with heat-resistant handles - 10 pieces', 'Kitchen', 149.99),
(45, 'Baking Sheet Set', 'Stainless steel baking sheets with silicone mats - set of 3', 'Kitchen', 36.99),
(46, 'Mixing Bowl Set', 'Stainless steel mixing bowls with lids - set of 5', 'Kitchen', 29.99),
(47, 'Measuring Cups', 'Stainless steel measuring cups and spoons set', 'Kitchen', 16.99),
(48, 'Can Opener', 'Electric can opener with smooth edge cutting', 'Kitchen', 24.99),
(49, 'Wine Opener', 'Electric wine opener with foil cutter', 'Kitchen', 29.99),
(50, 'Vegetable Peeler', 'Ergonomic stainless steel vegetable peeler set', 'Kitchen', 11.99),
(51, 'Garlic Press', 'Heavy-duty garlic press with easy-clean design', 'Kitchen', 14.99),
(52, 'Salad Spinner', 'Large salad spinner with pull cord mechanism', 'Kitchen', 19.99),
(53, 'Colander', 'Stainless steel colander with extended handles', 'Kitchen', 22.99),
(54, 'Oven Mitts', 'Heat-resistant oven mitts and pot holders set', 'Kitchen', 17.99),
(55, 'Dish Soap Dispenser', 'Automatic soap dispenser with touchless sensor', 'Kitchen', 26.99),
(56, 'LED Light Bulbs', 'Energy-efficient LED bulbs - pack of 4', 'Home', 24.99),
(57, 'Smart Plug', 'WiFi smart plug with app control and voice assistant compatibility', 'Electronics', 19.99),
(58, 'Extension Cord', 'Surge protector power strip with 12 outlets and USB ports', 'Electronics', 34.99),
(59, 'Flashlight', 'Rechargeable tactical flashlight with high lumens', 'Accessories', 29.99),
(60, 'Batteries Pack', 'AA and AAA alkaline batteries - variety pack', 'Accessories', 19.99),
(61, 'Alarm Clock', 'Digital alarm clock with USB charging ports and large display', 'Electronics', 24.99),
(62, 'Wall Clock', 'Modern silent wall clock with large numbers', 'Home', 29.99),
(63, 'Picture Frames', 'Photo frames set with matting - set of 5', 'Home', 34.99),
(64, 'Curtains', 'Blackout curtains with thermal insulation - set of 2', 'Home', 44.99),
(65, 'Throw Pillows', 'Decorative throw pillows with covers - set of 4', 'Home', 39.99),
(66, 'Area Rug', 'Modern geometric area rug for living room', 'Home', 129.99),
(67, 'Coat Rack', 'Wall-mounted coat rack with 5 hooks', 'Furniture', 34.99),
(68, 'Shoe Rack', '3-tier shoe organizer with stackable design', 'Furniture', 29.99),
(69, 'Storage Bins', 'Fabric storage bins with handles - set of 6', 'Home', 34.99),
(70, 'Hangers Pack', 'Velvet hangers non-slip with clips - pack of 50', 'Home', 24.99),
(71, 'Laundry Basket', 'Large collapsible laundry hamper with handles', 'Home', 19.99),
(72, 'Iron', 'Steam iron with anti-drip and auto shut-off', 'Home', 39.99),
(73, 'Ironing Board', 'Adjustable height ironing board with cotton cover', 'Home', 54.99),
(74, 'Vacuum Cleaner', 'Cordless stick vacuum with strong suction', 'Home', 179.99),
(75, 'Mop and Bucket', 'Spin mop with wringer bucket system', 'Home', 44.99),
(76, 'Broom and Dustpan', 'Upright broom and dustpan combo with long handle', 'Home', 24.99),
(77, 'Cleaning Gloves', 'Reusable cleaning gloves - pack of 3', 'Home', 11.99),
(78, 'Microfiber Cloths', 'Microfiber cleaning cloths - pack of 24', 'Home', 16.99),
(79, 'Spray Bottles', 'Empty spray bottles for cleaning solutions - set of 3', 'Home', 12.99),
(80, 'Toilet Brush', 'Toilet bowl brush with holder and drip tray', 'Home', 14.99),
(81, 'Shower Curtain', 'Waterproof shower curtain with hooks', 'Home', 19.99),
(82, 'Bath Mat', 'Memory foam bath mat with non-slip backing', 'Home', 24.99),
(83, 'Towel Set', 'Cotton towel set with bath, hand, and wash towels', 'Home', 44.99),
(84, 'Soap Dispenser', 'Stainless steel soap dispenser for bathroom', 'Home', 16.99),
(85, 'Toothbrush Holder', 'Multi-slot toothbrush holder with drainage', 'Home', 13.99),
(86, 'Shower Caddy', 'Rust-proof shower caddy with multiple shelves', 'Home', 29.99),
(87, 'Bathroom Scale', 'Digital bathroom scale with high precision', 'Wellness', 29.99),
(88, 'Mirror Makeup', 'LED lighted makeup mirror with magnification', 'Accessories', 39.99),
(89, 'Hair Dryer', 'Ionic hair dryer with multiple heat settings', 'Electronics', 59.99),
(90, 'Shaving Kit', 'Complete shaving kit with razor and stand', 'Wellness', 34.99),
(91, 'First Aid Kit', 'Complete first aid kit for home and travel', 'Wellness', 29.99),
(92, 'Thermometer Digital', 'Fast-reading digital thermometer with memory', 'Wellness', 19.99),
(93, 'Humidifier', 'Cool mist humidifier with essential oil tray', 'Wellness', 49.99),
(94, 'Dehumidifier', 'Compact dehumidifier for small spaces', 'Home', 79.99),
(95, 'Fan', 'Tower fan with remote control and oscillation', 'Home', 69.99),
(96, 'Space Heater', 'Portable space heater with thermostat and timer', 'Home', 59.99),
(97, 'Fire Extinguisher', 'Multi-purpose fire extinguisher for home use', 'Safety', 39.99),
(98, 'Smoke Detector', 'Photoelectric smoke alarm with 10-year battery', 'Safety', 24.99),
(99, 'Carbon Monoxide Detector', 'CO detector with digital display', 'Safety', 34.99),
(100, 'Door Lock Smart', 'Smart door lock with keypad and app control', 'Electronics', 149.99);

-- Note: In production, you would load the sqlite-vec extension and add actual embeddings:
-- .load ./vec0
-- UPDATE products SET embedding = vec_f32(random_vector(384)) WHERE id > 0;

-- For testing purposes without sqlite-vec, we'll create a mock similarity function
-- This allows the schema to work even without the extension loaded

-- Create metadata table for product statistics
CREATE TABLE IF NOT EXISTS product_stats (
    id INTEGER PRIMARY KEY,
    rating REAL DEFAULT 4.0,
    review_count INTEGER DEFAULT 0,
    in_stock BOOLEAN DEFAULT 1
);

-- Insert stats for all products
INSERT INTO product_stats (id, rating, review_count, in_stock)
SELECT id,
       3.5 + (ABS(RANDOM()) % 15) / 10.0,
       ABS(RANDOM()) % 500,
       1
FROM products;

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_products_category ON products(category);
CREATE INDEX IF NOT EXISTS idx_products_price ON products(price);

-- Verify data
SELECT COUNT(*) as total_products FROM products;
SELECT category, COUNT(*) as count FROM products GROUP BY category ORDER BY count DESC;
SELECT category,
       ROUND(AVG(price), 2) as avg_price,
       MIN(price) as min_price,
       MAX(price) as max_price
FROM products
GROUP BY category
ORDER BY avg_price DESC;

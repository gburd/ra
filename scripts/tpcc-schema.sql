-- TPC-C Schema for TPROC-C OLTP Benchmarking
--
-- Based on TPC-C specification v5.11
-- Scaled for warehouse_count warehouses.
--
-- Usage:
--   psql -d bench -v warehouse_count=10 -f scripts/tpcc-schema.sql
--
-- Load data (after generating with tpcc-mysql or similar):
--   psql -d bench -c "\copy warehouse FROM 'warehouse.csv' CSV"
--   psql -d bench -c "\copy district  FROM 'district.csv'  CSV"
--   ...
--
-- Approximate row counts at scale factor 1 (1 warehouse):
--   warehouse      1    district      10   customer    3,000
--   history    3,000    orders     3,000   new_order     900
--   order_line 30,000   item      100,000  stock       100,000

-- Drop existing tables
DROP TABLE IF EXISTS order_line  CASCADE;
DROP TABLE IF EXISTS new_order   CASCADE;
DROP TABLE IF EXISTS history     CASCADE;
DROP TABLE IF EXISTS orders      CASCADE;
DROP TABLE IF EXISTS customer    CASCADE;
DROP TABLE IF EXISTS stock       CASCADE;
DROP TABLE IF EXISTS item        CASCADE;
DROP TABLE IF EXISTS district    CASCADE;
DROP TABLE IF EXISTS warehouse   CASCADE;

-- ============================================================
-- Warehouse
-- ============================================================
CREATE TABLE warehouse (
    w_id       SMALLINT      NOT NULL PRIMARY KEY,
    w_name     VARCHAR(10),
    w_street_1 VARCHAR(20),
    w_street_2 VARCHAR(20),
    w_city     VARCHAR(20),
    w_state    CHAR(2),
    w_zip      CHAR(9),
    w_tax      DECIMAL(4,4),
    w_ytd      DECIMAL(12,2)
);

-- ============================================================
-- District (10 per warehouse)
-- ============================================================
CREATE TABLE district (
    d_id        SMALLINT       NOT NULL,
    d_w_id      SMALLINT       NOT NULL REFERENCES warehouse(w_id),
    d_name      VARCHAR(10),
    d_street_1  VARCHAR(20),
    d_street_2  VARCHAR(20),
    d_city      VARCHAR(20),
    d_state     CHAR(2),
    d_zip       CHAR(9),
    d_tax       DECIMAL(4,4),
    d_ytd       DECIMAL(12,2),
    d_next_o_id INT,
    PRIMARY KEY (d_w_id, d_id)
);

-- ============================================================
-- Customer (3000 per district)
-- ============================================================
CREATE TABLE customer (
    c_id           INT          NOT NULL,
    c_d_id         SMALLINT     NOT NULL,
    c_w_id         SMALLINT     NOT NULL,
    c_first        VARCHAR(16),
    c_middle       CHAR(2),
    c_last         VARCHAR(16),
    c_street_1     VARCHAR(20),
    c_street_2     VARCHAR(20),
    c_city         VARCHAR(20),
    c_state        CHAR(2),
    c_zip          CHAR(9),
    c_phone        CHAR(16),
    c_since        TIMESTAMP,
    c_credit       CHAR(2),
    c_credit_lim   DECIMAL(12,2),
    c_discount     DECIMAL(4,4),
    c_balance      DECIMAL(12,2),
    c_ytd_payment  DECIMAL(12,2),
    c_payment_cnt  SMALLINT,
    c_delivery_cnt SMALLINT,
    c_data         VARCHAR(500),
    PRIMARY KEY (c_w_id, c_d_id, c_id),
    FOREIGN KEY (c_w_id, c_d_id) REFERENCES district (d_w_id, d_id)
);

-- ============================================================
-- History (1 per initial customer; inserts only, never queried by PK)
-- ============================================================
CREATE TABLE history (
    h_c_id    INT,
    h_c_d_id  SMALLINT,
    h_c_w_id  SMALLINT,
    h_d_id    SMALLINT,
    h_w_id    SMALLINT,
    h_date    TIMESTAMP,
    h_amount  DECIMAL(6,2),
    h_data    VARCHAR(24)
);

-- ============================================================
-- Orders
-- ============================================================
CREATE TABLE orders (
    o_id         INT          NOT NULL,
    o_d_id       SMALLINT     NOT NULL,
    o_w_id       SMALLINT     NOT NULL,
    o_c_id       INT,
    o_entry_d    TIMESTAMP,
    o_carrier_id SMALLINT,
    o_ol_cnt     SMALLINT,
    o_all_local  SMALLINT,
    PRIMARY KEY (o_w_id, o_d_id, o_id),
    FOREIGN KEY (o_w_id, o_d_id, o_c_id)
        REFERENCES customer (c_w_id, c_d_id, c_id)
);

-- ============================================================
-- New-Order (subset of open orders awaiting delivery)
-- ============================================================
CREATE TABLE new_order (
    no_o_id  INT      NOT NULL,
    no_d_id  SMALLINT NOT NULL,
    no_w_id  SMALLINT NOT NULL,
    PRIMARY KEY (no_w_id, no_d_id, no_o_id),
    FOREIGN KEY (no_w_id, no_d_id, no_o_id)
        REFERENCES orders (o_w_id, o_d_id, o_id)
);

-- ============================================================
-- Item (100,000 items; static reference data)
-- ============================================================
CREATE TABLE item (
    i_id     INT          NOT NULL PRIMARY KEY,
    i_im_id  INT,
    i_name   VARCHAR(24),
    i_price  DECIMAL(5,2),
    i_data   VARCHAR(50)
);

-- ============================================================
-- Stock (100,000 per warehouse)
-- ============================================================
CREATE TABLE stock (
    s_i_id       INT      NOT NULL,
    s_w_id       SMALLINT NOT NULL,
    s_quantity   SMALLINT,
    s_dist_01    CHAR(24),
    s_dist_02    CHAR(24),
    s_dist_03    CHAR(24),
    s_dist_04    CHAR(24),
    s_dist_05    CHAR(24),
    s_dist_06    CHAR(24),
    s_dist_07    CHAR(24),
    s_dist_08    CHAR(24),
    s_dist_09    CHAR(24),
    s_dist_10    CHAR(24),
    s_ytd        INT,
    s_order_cnt  SMALLINT,
    s_remote_cnt SMALLINT,
    s_data       VARCHAR(50),
    PRIMARY KEY (s_w_id, s_i_id),
    FOREIGN KEY (s_w_id) REFERENCES warehouse(w_id),
    FOREIGN KEY (s_i_id) REFERENCES item(i_id)
);

-- ============================================================
-- Order-Line (avg 10 per order)
-- ============================================================
CREATE TABLE order_line (
    ol_o_id        INT          NOT NULL,
    ol_d_id        SMALLINT     NOT NULL,
    ol_w_id        SMALLINT     NOT NULL,
    ol_number      SMALLINT     NOT NULL,
    ol_i_id        INT,
    ol_supply_w_id SMALLINT,
    ol_delivery_d  TIMESTAMP,
    ol_quantity    SMALLINT,
    ol_amount      DECIMAL(6,2),
    ol_dist_info   CHAR(24),
    PRIMARY KEY (ol_w_id, ol_d_id, ol_o_id, ol_number),
    FOREIGN KEY (ol_w_id, ol_d_id, ol_o_id)
        REFERENCES orders (o_w_id, o_d_id, o_id)
);

-- ============================================================
-- Indexes (TPC-C spec section 1.4)
-- ============================================================

-- customer: secondary index on (c_w_id, c_d_id, c_last) for Payment by name
CREATE INDEX idx_customer_last ON customer (c_w_id, c_d_id, c_last);

-- orders: secondary index for most-recent order lookup
CREATE INDEX idx_orders_customer ON orders (o_w_id, o_d_id, o_c_id, o_id DESC);

-- new_order: efficient MIN(no_o_id) lookup per district
CREATE INDEX idx_new_order_min ON new_order (no_w_id, no_d_id, no_o_id);

-- order_line: lookup by order and item for Stock-Level
CREATE INDEX idx_ol_item ON order_line (ol_w_id, ol_d_id, ol_o_id, ol_i_id);

-- ============================================================
-- ANALYZE to collect planner statistics
-- ============================================================
ANALYZE warehouse;
ANALYZE district;
ANALYZE customer;
ANALYZE orders;
ANALYZE new_order;
ANALYZE item;
ANALYZE stock;
ANALYZE order_line;

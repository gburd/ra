-- TPC-H Schema Data Generation (Scale 0.01)
-- Generates approximately: 1500 customers, 15000 orders, 60000 lineitems

\echo 'Generating TPC-H test data (scale 0.01)...'

-- Generate 1,500 customers
INSERT INTO customer (c_custkey, c_name, c_address, c_nationkey, c_phone, c_acctbal, c_mktsegment, c_comment)
SELECT
  i,
  'Customer#' || LPAD(i::text, 9, '0'),
  'Address ' || i,
  (random() * 24)::int + 1,
  '1' || LPAD((random() * 9999999999)::bigint::text, 11, '0'),
  (random() * 10000 - 1000)::decimal(15,2),
  (ARRAY['AUTOMOBILE', 'BUILDING', 'FURNITURE', 'HOUSEHOLD', 'MACHINERY'])[1 + (random() * 4)::int],
  'Comment for customer ' || i
FROM generate_series(1, 1500) AS i;

\echo 'Generated 1,500 customers'

-- Generate 15,000 orders
INSERT INTO orders (o_orderkey, o_custkey, o_orderstatus, o_totalprice, o_orderdate, o_orderpriority, o_clerk, o_shippriority, o_comment)
SELECT
  i,
  1 + (random() * 1499)::int,
  (ARRAY['O', 'F', 'P'])[1 + (random() * 2)::int],
  (random() * 500000)::decimal(15,2),
  DATE '1992-01-01' + (random() * 2556)::int,
  (ARRAY['1-URGENT', '2-HIGH', '3-MEDIUM', '4-NOT SPECIFIED', '5-LOW'])[1 + (random() * 4)::int],
  'Clerk#' || LPAD((random() * 1000)::int::text, 9, '0'),
  (random() * 5)::int,
  'Comment for order ' || i
FROM generate_series(1, 15000) AS i;

\echo 'Generated 15,000 orders'

-- Generate 60,000 lineitems (average 4 per order)
INSERT INTO lineitem (
  l_orderkey, l_partkey, l_suppkey, l_linenumber,
  l_quantity, l_extendedprice, l_discount, l_tax,
  l_returnflag, l_linestatus, l_shipdate, l_commitdate, l_receiptdate,
  l_shipinstruct, l_shipmode, l_comment
)
SELECT
  1 + (random() * 14999)::int,
  1 + (random() * 19999)::int,
  1 + (random() * 999)::int,
  1 + (random() * 6)::int,
  (1 + random() * 50)::decimal(15,2),
  (random() * 100000)::decimal(15,2),
  (random() * 0.1)::decimal(15,2),
  (random() * 0.08)::decimal(15,2),
  (ARRAY['A', 'N', 'R'])[1 + (random() * 2)::int],
  (ARRAY['O', 'F'])[1 + (random() * 1)::int],
  DATE '1992-01-01' + (random() * 2556)::int,
  DATE '1992-01-01' + (random() * 2556)::int,
  DATE '1992-01-01' + (random() * 2556)::int,
  (ARRAY['DELIVER IN PERSON', 'COLLECT COD', 'NONE', 'TAKE BACK RETURN'])[1 + (random() * 3)::int],
  (ARRAY['TRUCK', 'MAIL', 'REG AIR', 'SHIP', 'RAIL', 'FOB', 'AIR'])[1 + (random() * 6)::int],
  'Comment for lineitem'
FROM generate_series(1, 60000) AS i;

\echo 'Generated 60,000 lineitems'

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_customer_nationkey ON customer(c_nationkey);
CREATE INDEX IF NOT EXISTS idx_orders_custkey ON orders(o_custkey);
CREATE INDEX IF NOT EXISTS idx_orders_orderdate ON orders(o_orderdate);
CREATE INDEX IF NOT EXISTS idx_lineitem_orderkey ON lineitem(l_orderkey);
CREATE INDEX IF NOT EXISTS idx_lineitem_shipdate ON lineitem(l_shipdate);

\echo 'TPC-H test data generation complete'

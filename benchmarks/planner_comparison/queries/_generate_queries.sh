#!/usr/bin/env bash
# Generate all query files

# Simple queries (10)
cat > simple/01_simple_scan.sql << 'SQL'
-- Simple table scan with filter
SELECT * FROM lineitem WHERE l_shipdate >= '1998-01-01';
SQL

cat > simple/02_simple_aggregate.sql << 'SQL'
-- Simple aggregation
SELECT COUNT(*), SUM(l_quantity), AVG(l_extendedprice)
FROM lineitem;
SQL

cat > simple/03_group_by.sql << 'SQL'
-- GROUP BY with aggregates
SELECT l_returnflag, l_linestatus,
       COUNT(*) as count,
       SUM(l_quantity) as sum_qty,
       AVG(l_extendedprice) as avg_price
FROM lineitem
GROUP BY l_returnflag, l_linestatus;
SQL

cat > simple/04_filter_aggregate.sql << 'SQL'
-- Filter with aggregate
SELECT l_shipmode, COUNT(*) as order_count
FROM lineitem
WHERE l_shipdate >= '1997-01-01'
  AND l_shipdate < '1998-01-01'
GROUP BY l_shipmode
ORDER BY l_shipmode;
SQL

cat > simple/05_selective_filter.sql << 'SQL'
-- Highly selective filter (1%)
SELECT * FROM lineitem
WHERE l_quantity < 2
  AND l_discount > 0.09
  AND l_discount < 0.11;
SQL

cat > simple/06_order_limit.sql << 'SQL'
-- ORDER BY with LIMIT
SELECT l_orderkey, l_partkey, l_extendedprice
FROM lineitem
ORDER BY l_extendedprice DESC
LIMIT 100;
SQL

cat > simple/07_distinct_count.sql << 'SQL'
-- DISTINCT aggregation
SELECT COUNT(DISTINCT l_partkey) as distinct_parts
FROM lineitem;
SQL

cat > simple/08_having_clause.sql << 'SQL'
-- HAVING clause
SELECT l_returnflag, COUNT(*) as count
FROM lineitem
GROUP BY l_returnflag
HAVING COUNT(*) > 1000000;
SQL

cat > simple/09_multiple_filters.sql << 'SQL'
-- Multiple AND/OR filters
SELECT l_orderkey, l_linenumber
FROM lineitem
WHERE (l_quantity < 10 OR l_discount > 0.05)
  AND l_shipdate >= '1997-01-01';
SQL

cat > simple/10_offset.sql << 'SQL'
-- OFFSET pagination
SELECT o_orderkey, o_custkey, o_totalprice
FROM orders
ORDER BY o_orderdate
LIMIT 100 OFFSET 1000;
SQL

echo "Created 10 simple queries"

# Basic joins (15)
cat > basic_joins/01_inner_join.sql << 'SQL'
-- Simple INNER JOIN
SELECT o.o_orderkey, o.o_orderdate, c.c_name
FROM orders o
INNER JOIN customer c ON o.o_custkey = c.c_custkey
WHERE o.o_orderdate >= '1998-01-01';
SQL

cat > basic_joins/02_left_join.sql << 'SQL'
-- LEFT OUTER JOIN
SELECT c.c_custkey, c.c_name, COUNT(o.o_orderkey) as order_count
FROM customer c
LEFT JOIN orders o ON c.c_custkey = o.o_custkey
GROUP BY c.c_custkey, c.c_name;
SQL

cat > basic_joins/03_right_join.sql << 'SQL'
-- RIGHT OUTER JOIN
SELECT o.o_orderkey, c.c_name
FROM orders o
RIGHT JOIN customer c ON o.o_custkey = c.c_custkey;
SQL

cat > basic_joins/04_equi_join_filter.sql << 'SQL'
-- INNER JOIN with filters on both sides
SELECT l.l_orderkey, l.l_linenumber, o.o_orderdate
FROM lineitem l
JOIN orders o ON l.l_orderkey = o.o_orderkey
WHERE l.l_quantity > 40
  AND o.o_totalprice > 100000;
SQL

cat > basic_joins/05_three_table_join.sql << 'SQL'
-- Three-table star join
SELECT c.c_name, o.o_orderkey, l.l_quantity
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE c.c_nationkey = 5;
SQL

cat > basic_joins/06_foreign_key.sql << 'SQL'
-- Foreign key join
SELECT s.s_name, n.n_name
FROM supplier s
JOIN nation n ON s.s_nationkey = n.n_nationkey
WHERE n.n_regionkey = 1;
SQL

cat > basic_joins/07_multi_predicate_join.sql << 'SQL'
-- Join with multiple predicates
SELECT p.p_partkey, ps.ps_supplycost
FROM part p
JOIN partsupp ps ON p.p_partkey = ps.ps_partkey
WHERE p.p_size > 30
  AND ps.ps_availqty > 5000;
SQL

cat > basic_joins/08_cross_product.sql << 'SQL'
-- Small cross product (nation x region)
SELECT n.n_name, r.r_name
FROM nation n, region r
WHERE n.n_regionkey = r.r_regionkey;
SQL

cat > basic_joins/09_join_aggregate.sql << 'SQL'
-- Join with aggregation
SELECT o.o_orderpriority, COUNT(*) as order_count
FROM orders o
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE l.l_commitdate < l.l_receiptdate
GROUP BY o.o_orderpriority;
SQL

cat > basic_joins/10_self_join.sql << 'SQL'
-- Self join
SELECT l1.l_orderkey, l1.l_partkey, l2.l_partkey
FROM lineitem l1
JOIN lineitem l2 ON l1.l_orderkey = l2.l_orderkey
WHERE l1.l_linenumber < l2.l_linenumber
LIMIT 1000;
SQL

cat > basic_joins/11_dimension_table.sql << 'SQL'
-- Dimension table join
SELECT n.n_name, COUNT(*) as supplier_count
FROM supplier s
JOIN nation n ON s.s_nationkey = n.n_nationkey
GROUP BY n.n_name
ORDER BY supplier_count DESC;
SQL

cat > basic_joins/12_join_with_in.sql << 'SQL'
-- Join with IN clause
SELECT c.c_name, o.o_orderdate
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
WHERE c.c_nationkey IN (1, 5, 10);
SQL

cat > basic_joins/13_non_equi_join.sql << 'SQL'
-- Non-equi join
SELECT l1.l_orderkey, l1.l_quantity, l2.l_quantity
FROM lineitem l1
JOIN lineitem l2 ON l1.l_orderkey = l2.l_orderkey
  AND l1.l_quantity < l2.l_quantity
WHERE l1.l_linenumber = 1
LIMIT 100;
SQL

cat > basic_joins/14_join_distinct.sql << 'SQL'
-- Join with DISTINCT
SELECT DISTINCT c.c_nationkey, o.o_orderpriority
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey;
SQL

cat > basic_joins/15_join_computed.sql << 'SQL'
-- Join with computed columns
SELECT o.o_orderkey,
       l.l_extendedprice * (1 - l.l_discount) as revenue
FROM orders o
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE o.o_orderdate >= '1997-01-01'
  AND o.o_orderdate < '1998-01-01';
SQL

echo "Created 15 basic join queries"

# Add more categories as needed...
echo "Query generation complete!"

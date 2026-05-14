-- UNION ALL: all transactions (orders and returns)
SELECT o_orderkey AS transaction_id, o_custkey, o_totalprice AS amount, 'order' AS type
FROM orders
WHERE o_orderdate >= '1998-01-01'
UNION ALL
SELECT l_orderkey AS transaction_id, l_suppkey, l_extendedprice AS amount, 'return' AS type
FROM lineitem
WHERE l_returnflag = 'R'
  AND l_shipdate >= '1998-01-01';

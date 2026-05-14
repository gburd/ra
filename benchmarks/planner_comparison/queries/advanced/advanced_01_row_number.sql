-- ROW_NUMBER window function: rank orders per customer
SELECT o_custkey, o_orderkey, o_totalprice,
       ROW_NUMBER() OVER (PARTITION BY o_custkey ORDER BY o_totalprice DESC) AS rn
FROM orders
WHERE o_orderdate >= '1996-01-01';

-- Join with computed columns
SELECT o.o_orderkey,
       l.l_extendedprice * (1 - l.l_discount) as revenue
FROM orders o
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
WHERE o.o_orderdate >= '1997-01-01'
  AND o.o_orderdate < '1998-01-01';

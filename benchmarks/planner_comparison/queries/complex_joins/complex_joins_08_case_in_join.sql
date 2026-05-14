-- Join with CASE expression in projection
SELECT o.o_orderkey,
       CASE
           WHEN o.o_orderpriority = '1-URGENT' THEN 'HIGH'
           WHEN o.o_orderpriority = '2-HIGH' THEN 'HIGH'
           ELSE 'NORMAL'
       END AS priority_group,
       SUM(l.l_extendedprice * (1 - l.l_discount)) AS revenue
FROM orders o
JOIN lineitem l ON o.o_orderkey = l.l_orderkey
GROUP BY o.o_orderkey, o.o_orderpriority;

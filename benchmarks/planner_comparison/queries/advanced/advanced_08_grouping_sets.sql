-- GROUPING SETS for multiple aggregation levels
SELECT n.n_name, EXTRACT(YEAR FROM o.o_orderdate) AS order_year,
       o.o_orderpriority, COUNT(*) AS order_count,
       SUM(o.o_totalprice) AS total_value
FROM orders o
JOIN customer c ON o.o_custkey = c.c_custkey
JOIN nation n ON c.c_nationkey = n.n_nationkey
WHERE o.o_orderdate >= '1995-01-01' AND o.o_orderdate < '1997-01-01'
GROUP BY GROUPING SETS (
    (n.n_name, EXTRACT(YEAR FROM o.o_orderdate)),
    (n.n_name, o.o_orderpriority),
    (n.n_name)
);

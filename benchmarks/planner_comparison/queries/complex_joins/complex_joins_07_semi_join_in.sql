-- Semi-join via IN: orders from European customers
SELECT o.o_orderkey, o.o_orderdate, o.o_totalprice
FROM orders o
WHERE o.o_custkey IN (
    SELECT c.c_custkey FROM customer c
    JOIN nation n ON c.c_nationkey = n.n_nationkey
    JOIN region r ON n.n_regionkey = r.r_regionkey
    WHERE r.r_name = 'EUROPE'
);

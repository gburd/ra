-- Correlated scalar subquery in WHERE
SELECT o.o_orderkey, o.o_custkey, o.o_totalprice
FROM orders o
WHERE o.o_totalprice > (
    SELECT AVG(o2.o_totalprice) FROM orders o2
    WHERE o2.o_custkey = o.o_custkey
);

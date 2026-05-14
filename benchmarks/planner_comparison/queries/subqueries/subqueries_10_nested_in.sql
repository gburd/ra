-- Nested IN subqueries: orders from customers in Asian nations
SELECT o.o_orderkey, o.o_totalprice
FROM orders o
WHERE o.o_custkey IN (
    SELECT c.c_custkey FROM customer c
    WHERE c.c_nationkey IN (
        SELECT n.n_nationkey FROM nation n
        WHERE n.n_regionkey IN (
            SELECT r.r_regionkey FROM region r
            WHERE r.r_name = 'ASIA'
        )
    )
);

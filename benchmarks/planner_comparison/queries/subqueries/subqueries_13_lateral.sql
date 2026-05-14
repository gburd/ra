-- Lateral join: top 3 orders per customer
SELECT c.c_name, top_orders.o_orderkey, top_orders.o_totalprice
FROM customer c,
LATERAL (
    SELECT o.o_orderkey, o.o_totalprice
    FROM orders o
    WHERE o.o_custkey = c.c_custkey
    ORDER BY o.o_totalprice DESC
    LIMIT 3
) top_orders
WHERE c.c_acctbal > 1000;

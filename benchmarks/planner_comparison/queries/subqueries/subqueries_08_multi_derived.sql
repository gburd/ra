-- Multiple derived tables joined
SELECT top_cust.c_name, top_cust.total_orders, recent.recent_total
FROM (
    SELECT c.c_custkey, c.c_name, COUNT(*) AS total_orders
    FROM customer c
    JOIN orders o ON c.c_custkey = o.o_custkey
    GROUP BY c.c_custkey, c.c_name
    HAVING COUNT(*) > 15
) top_cust
JOIN (
    SELECT o.o_custkey, SUM(o.o_totalprice) AS recent_total
    FROM orders o
    WHERE o.o_orderdate >= '1997-01-01'
    GROUP BY o.o_custkey
) recent ON top_cust.c_custkey = recent.o_custkey
ORDER BY recent.recent_total DESC
LIMIT 10;

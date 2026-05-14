-- Nested aggregate via subquery: customers above average order value
SELECT c_custkey, c_name, avg_order_value
FROM (
    SELECT c.c_custkey, c.c_name, AVG(o.o_totalprice) AS avg_order_value
    FROM customer c
    JOIN orders o ON c.c_custkey = o.o_custkey
    GROUP BY c.c_custkey, c.c_name
) cust_avg
WHERE avg_order_value > (
    SELECT AVG(o_totalprice) FROM orders
);

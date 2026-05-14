-- Derived table (subquery in FROM)
SELECT nation_orders.n_name, nation_orders.order_count, nation_orders.avg_value
FROM (
    SELECT n.n_name, COUNT(o.o_orderkey) AS order_count,
           AVG(o.o_totalprice) AS avg_value
    FROM nation n
    JOIN customer c ON n.n_nationkey = c.c_nationkey
    JOIN orders o ON c.c_custkey = o.o_custkey
    GROUP BY n.n_name
) nation_orders
WHERE nation_orders.order_count > 1000
ORDER BY nation_orders.avg_value DESC;

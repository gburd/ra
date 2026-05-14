-- LAG and LEAD: order-to-order comparison
SELECT o_custkey, o_orderkey, o_orderdate, o_totalprice,
       LAG(o_totalprice) OVER (PARTITION BY o_custkey ORDER BY o_orderdate) AS prev_order_value,
       LEAD(o_totalprice) OVER (PARTITION BY o_custkey ORDER BY o_orderdate) AS next_order_value
FROM orders
WHERE o_custkey <= 100;

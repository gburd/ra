-- GROUP BY with expressions
SELECT EXTRACT(YEAR FROM o_orderdate) AS order_year,
       EXTRACT(MONTH FROM o_orderdate) AS order_month,
       COUNT(*) AS order_count,
       AVG(o_totalprice) AS avg_price
FROM orders
WHERE o_orderdate >= '1993-01-01'
GROUP BY EXTRACT(YEAR FROM o_orderdate), EXTRACT(MONTH FROM o_orderdate)
ORDER BY order_year, order_month;

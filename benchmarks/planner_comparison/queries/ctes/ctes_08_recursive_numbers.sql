-- Recursive CTE: generate number series for date ranges
WITH RECURSIVE months AS (
    SELECT 1 AS month_num
    UNION ALL
    SELECT month_num + 1
    FROM months
    WHERE month_num < 12
)
SELECT m.month_num, COUNT(o.o_orderkey) AS order_count
FROM months m
LEFT JOIN orders o ON EXTRACT(MONTH FROM o.o_orderdate) = m.month_num
    AND EXTRACT(YEAR FROM o.o_orderdate) = 1996
GROUP BY m.month_num
ORDER BY m.month_num;

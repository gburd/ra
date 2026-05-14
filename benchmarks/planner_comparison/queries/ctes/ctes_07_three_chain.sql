-- Three CTEs in a chain
WITH yearly_revenue AS (
    SELECT EXTRACT(YEAR FROM o_orderdate) AS year,
           o_custkey, SUM(o_totalprice) AS annual_revenue
    FROM orders
    GROUP BY EXTRACT(YEAR FROM o_orderdate), o_custkey
),
customer_growth AS (
    SELECT y1.o_custkey,
           y1.year AS year1, y1.annual_revenue AS rev1,
           y2.year AS year2, y2.annual_revenue AS rev2
    FROM yearly_revenue y1
    JOIN yearly_revenue y2 ON y1.o_custkey = y2.o_custkey
        AND y2.year = y1.year + 1
),
growing_customers AS (
    SELECT o_custkey
    FROM customer_growth
    WHERE rev2 > rev1 * 1.2
    GROUP BY o_custkey
    HAVING COUNT(*) >= 2
)
SELECT c.c_name, c.c_acctbal
FROM growing_customers gc
JOIN customer c ON gc.o_custkey = c.c_custkey
ORDER BY c.c_acctbal DESC;

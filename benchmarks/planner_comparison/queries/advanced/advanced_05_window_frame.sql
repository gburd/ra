-- Window frame: running total and moving average
SELECT o_custkey, o_orderkey, o_orderdate, o_totalprice,
       SUM(o_totalprice) OVER (
           PARTITION BY o_custkey
           ORDER BY o_orderdate
           ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
       ) AS running_total,
       AVG(o_totalprice) OVER (
           PARTITION BY o_custkey
           ORDER BY o_orderdate
           ROWS BETWEEN 2 PRECEDING AND CURRENT ROW
       ) AS moving_avg_3
FROM orders
WHERE o_custkey <= 50;

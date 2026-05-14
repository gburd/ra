-- Window functions over joined result with complex frame
SELECT c.c_name, o.o_orderdate, o.o_totalprice,
       RANK() OVER (PARTITION BY c.c_nationkey ORDER BY o.o_totalprice DESC) AS nation_rank,
       SUM(o.o_totalprice) OVER (
           PARTITION BY c.c_custkey
           ORDER BY o.o_orderdate
           ROWS BETWEEN 3 PRECEDING AND 1 FOLLOWING
       ) AS windowed_sum,
       o.o_totalprice - LAG(o.o_totalprice) OVER (
           PARTITION BY c.c_custkey ORDER BY o.o_orderdate
       ) AS price_change
FROM customer c
JOIN orders o ON c.c_custkey = o.o_custkey
WHERE c.c_nationkey = 15;

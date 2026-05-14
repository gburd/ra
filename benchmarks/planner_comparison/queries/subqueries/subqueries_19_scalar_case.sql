-- Scalar subquery inside CASE expression
SELECT o.o_orderkey, o.o_totalprice,
       CASE
           WHEN o.o_totalprice > (SELECT AVG(o2.o_totalprice) FROM orders o2) * 2
           THEN 'HIGH'
           WHEN o.o_totalprice > (SELECT AVG(o2.o_totalprice) FROM orders o2)
           THEN 'MEDIUM'
           ELSE 'LOW'
       END AS price_tier
FROM orders o
WHERE o.o_orderdate >= '1997-01-01'
LIMIT 100;

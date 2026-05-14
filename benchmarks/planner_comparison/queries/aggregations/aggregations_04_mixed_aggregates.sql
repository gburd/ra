-- Multiple aggregate functions combined
SELECT l_returnflag,
       COUNT(*) AS cnt,
       SUM(l_quantity) AS sum_qty,
       AVG(l_extendedprice) AS avg_price,
       MIN(l_discount) AS min_disc,
       MAX(l_discount) AS max_disc,
       SUM(l_extendedprice * (1 - l_discount)) AS sum_charge
FROM lineitem
GROUP BY l_returnflag;

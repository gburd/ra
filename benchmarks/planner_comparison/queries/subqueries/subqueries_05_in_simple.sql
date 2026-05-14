-- Simple IN subquery
SELECT p.p_partkey, p.p_name, p.p_retailprice
FROM part p
WHERE p.p_partkey IN (
    SELECT l.l_partkey FROM lineitem l
    WHERE l.l_quantity > 40
);

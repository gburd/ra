-- Anti-join via NOT IN: parts never ordered
SELECT p.p_partkey, p.p_name, p.p_retailprice
FROM part p
WHERE p.p_partkey NOT IN (
    SELECT l.l_partkey FROM lineitem l
);

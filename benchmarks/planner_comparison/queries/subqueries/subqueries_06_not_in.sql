-- NOT IN subquery: nations without any suppliers
SELECT n.n_nationkey, n.n_name
FROM nation n
WHERE n.n_nationkey NOT IN (
    SELECT s.s_nationkey FROM supplier s
);

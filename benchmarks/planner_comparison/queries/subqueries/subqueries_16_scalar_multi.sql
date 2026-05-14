-- Multiple scalar subqueries in SELECT
SELECT p.p_partkey, p.p_name,
       (SELECT MIN(ps.ps_supplycost) FROM partsupp ps WHERE ps.ps_partkey = p.p_partkey) AS min_cost,
       (SELECT MAX(ps.ps_supplycost) FROM partsupp ps WHERE ps.ps_partkey = p.p_partkey) AS max_cost,
       (SELECT COUNT(*) FROM partsupp ps WHERE ps.ps_partkey = p.p_partkey) AS supplier_count
FROM part p
WHERE p.p_size > 20;

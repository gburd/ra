-- Dimension table join
SELECT n.n_name, COUNT(*) as supplier_count
FROM supplier s
JOIN nation n ON s.s_nationkey = n.n_nationkey
GROUP BY n.n_name
ORDER BY supplier_count DESC;

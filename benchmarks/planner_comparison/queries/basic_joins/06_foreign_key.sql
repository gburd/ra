-- Foreign key join
SELECT s.s_name, n.n_name
FROM supplier s
JOIN nation n ON s.s_nationkey = n.n_nationkey
WHERE n.n_regionkey = 1;

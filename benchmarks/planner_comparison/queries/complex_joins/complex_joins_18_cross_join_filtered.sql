-- Cross join with selective filter (nation pairs)
SELECT n1.n_name AS nation1, n2.n_name AS nation2
FROM nation n1
JOIN nation n2 ON n1.n_nationkey <> n2.n_nationkey
WHERE n1.n_regionkey = n2.n_regionkey
ORDER BY n1.n_name, n2.n_name;

-- Snowflake join: supplier through nation and region
SELECT s.s_name, n.n_name, r.r_name, ps.ps_supplycost, p.p_name
FROM supplier s
JOIN nation n ON s.s_nationkey = n.n_nationkey
JOIN region r ON n.n_regionkey = r.r_regionkey
JOIN partsupp ps ON s.s_suppkey = ps.ps_suppkey
JOIN part p ON ps.ps_partkey = p.p_partkey
WHERE r.r_name = 'EUROPE'
  AND p.p_size = 15;

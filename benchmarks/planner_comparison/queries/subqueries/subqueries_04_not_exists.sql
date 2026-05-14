-- NOT EXISTS: suppliers with no parts in a given region
SELECT s.s_suppkey, s.s_name
FROM supplier s
WHERE NOT EXISTS (
    SELECT 1 FROM partsupp ps
    JOIN part p ON ps.ps_partkey = p.p_partkey
    WHERE ps.ps_suppkey = s.s_suppkey
      AND p.p_type LIKE '%BRASS%'
);

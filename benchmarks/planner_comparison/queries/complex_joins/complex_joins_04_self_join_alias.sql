-- Self-join with aliases: find parts supplied by multiple suppliers
SELECT ps1.ps_partkey, ps1.ps_suppkey AS supp1, ps2.ps_suppkey AS supp2,
       ps1.ps_supplycost AS cost1, ps2.ps_supplycost AS cost2
FROM partsupp ps1
JOIN partsupp ps2 ON ps1.ps_partkey = ps2.ps_partkey
WHERE ps1.ps_suppkey < ps2.ps_suppkey
  AND ps1.ps_supplycost > ps2.ps_supplycost * 2;

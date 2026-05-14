-- MERGE statement: upsert pattern
MERGE INTO partsupp target
USING (
    SELECT l_partkey, l_suppkey, SUM(l_quantity) AS total_qty
    FROM lineitem
    WHERE l_shipdate >= '1998-01-01'
    GROUP BY l_partkey, l_suppkey
) source ON target.ps_partkey = source.l_partkey
    AND target.ps_suppkey = source.l_suppkey
WHEN MATCHED THEN
    UPDATE SET ps_availqty = target.ps_availqty - source.total_qty
WHEN NOT MATCHED THEN
    INSERT (ps_partkey, ps_suppkey, ps_availqty, ps_supplycost, ps_comment)
    VALUES (source.l_partkey, source.l_suppkey, source.total_qty, 0, 'auto-inserted');

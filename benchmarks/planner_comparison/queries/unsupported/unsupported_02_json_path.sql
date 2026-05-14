-- JSON path query: extract from JSON columns
SELECT o_orderkey, o_comment,
       o_comment::jsonb -> 'metadata' ->> 'priority' AS json_priority
FROM orders
WHERE o_comment::jsonb @> '{"urgent": true}'
LIMIT 50;

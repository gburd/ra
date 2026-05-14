-- Three-way UNION with different sources
SELECT n_nationkey AS key, n_name AS name, 'nation' AS source
FROM nation
UNION
SELECT r_regionkey AS key, r_name AS name, 'region' AS source
FROM region
UNION
SELECT s_suppkey AS key, s_name AS name, 'supplier' AS source
FROM supplier
WHERE s_suppkey <= 25;

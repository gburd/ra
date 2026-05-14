-- PIVOT: transform rows to columns (non-standard SQL extension)
SELECT *
FROM (
    SELECT n_regionkey, n_name
    FROM nation
) src
PIVOT (
    COUNT(n_name)
    FOR n_regionkey IN (0, 1, 2, 3, 4)
) AS pvt;

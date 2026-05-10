-- TPC-DS Query 39 (Part 1 and Part 2)
-- Compute monthly inventory for specific items within tolerance

-- Part 1
WITH inv AS (
    SELECT w_warehouse_name,
           w_warehouse_sk,
           i_item_sk,
           d_moy,
           stdev,
           mean,
           CASE mean WHEN 0 THEN NULL
                     ELSE stdev / mean END AS cov
    FROM (SELECT w_warehouse_name,
                 w_warehouse_sk,
                 i_item_sk,
                 d_moy,
                 stddev_samp(inv_quantity_on_hand) AS stdev,
                 avg(inv_quantity_on_hand) AS mean
          FROM inventory
          JOIN item ON inv_item_sk = i_item_sk
          JOIN warehouse ON inv_warehouse_sk = w_warehouse_sk
          JOIN date_dim ON inv_date_sk = d_date_sk
          WHERE d_year = 2001
          GROUP BY w_warehouse_name, w_warehouse_sk, i_item_sk, d_moy) foo
    WHERE CASE mean WHEN 0 THEN 0
                    ELSE stdev / mean END > 1
)
SELECT inv1.w_warehouse_sk,
       inv1.i_item_sk,
       inv1.d_moy,
       inv1.mean,
       inv1.cov,
       inv2.w_warehouse_sk,
       inv2.i_item_sk,
       inv2.d_moy,
       inv2.mean,
       inv2.cov
FROM inv inv1
JOIN inv inv2 ON inv1.i_item_sk = inv2.i_item_sk
  AND inv1.w_warehouse_sk = inv2.w_warehouse_sk
WHERE inv1.d_moy = 1
  AND inv2.d_moy = 1 + 1
ORDER BY inv1.w_warehouse_sk, inv1.i_item_sk, inv1.d_moy,
         inv1.mean, inv1.cov, inv2.d_moy, inv2.mean, inv2.cov;

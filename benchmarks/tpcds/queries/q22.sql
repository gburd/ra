-- TPC-DS Query 22
-- Count orders placed from same city/county/state/zip in a given year
-- using inventory and date dimensions.
SELECT
    i_product_name,
    i_brand,
    i_class,
    i_category,
    AVG(inv_quantity_on_hand) AS qoh
FROM inventory
JOIN date_dim ON inv_date_sk = d_date_sk
JOIN item ON inv_item_sk = i_item_sk
JOIN warehouse ON inv_warehouse_sk = w_warehouse_sk
WHERE d_month_seq BETWEEN 1200 AND 1211
GROUP BY ROLLUP(i_product_name, i_brand, i_class, i_category)
ORDER BY qoh, i_product_name, i_brand, i_class, i_category
LIMIT 100;

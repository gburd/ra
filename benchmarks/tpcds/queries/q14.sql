-- TPC-DS Query 14
-- Find items sold in both stores and catalogs with brands in specific
-- categories (part 1 and part 2 combined).
WITH cross_items AS (
    SELECT i_item_sk AS ss_item_sk
    FROM item
    WHERE i_brand_id IN (
        SELECT i_brand_id FROM item
        WHERE i_category IN ('Women', 'Men', 'Electronics')
        INTERSECT
        SELECT i_brand_id FROM item
        WHERE i_category IN ('Women', 'Men', 'Electronics')
    )
),
avg_sales AS (
    SELECT AVG(quantity * list_price) AS average_sales
    FROM (
        SELECT ss_quantity AS quantity, ss_list_price AS list_price
        FROM store_sales JOIN date_dim ON ss_sold_date_sk = d_date_sk
        WHERE d_year BETWEEN 1999 AND 2001
        UNION ALL
        SELECT cs_quantity AS quantity, cs_list_price AS list_price
        FROM catalog_sales JOIN date_dim ON cs_sold_date_sk = d_date_sk
        WHERE d_year BETWEEN 1999 AND 2001
        UNION ALL
        SELECT ws_quantity AS quantity, ws_list_price AS list_price
        FROM web_sales JOIN date_dim ON ws_sold_date_sk = d_date_sk
        WHERE d_year BETWEEN 1999 AND 2001
    ) x
)
-- Part 1
SELECT
    channel,
    i_brand_id,
    i_class_id,
    i_category_id,
    SUM(sales) AS total_sales,
    SUM(number_sales) AS total_number_sales
FROM (
    SELECT 'store' AS channel, i_brand_id, i_class_id, i_category_id,
           SUM(ss_quantity * ss_list_price) AS sales, COUNT(*) AS number_sales
    FROM store_sales
    JOIN item ON ss_item_sk = i_item_sk
    JOIN date_dim ON ss_sold_date_sk = d_date_sk
    WHERE ss_item_sk IN (SELECT ss_item_sk FROM cross_items)
      AND d_year = 1999 + 2
    GROUP BY i_brand_id, i_class_id, i_category_id
    HAVING SUM(ss_quantity * ss_list_price) > (SELECT average_sales FROM avg_sales)
    UNION ALL
    SELECT 'catalog' AS channel, i_brand_id, i_class_id, i_category_id,
           SUM(cs_quantity * cs_list_price) AS sales, COUNT(*) AS number_sales
    FROM catalog_sales
    JOIN item ON cs_item_sk = i_item_sk
    JOIN date_dim ON cs_sold_date_sk = d_date_sk
    WHERE cs_item_sk IN (SELECT ss_item_sk FROM cross_items)
      AND d_year = 1999 + 2
    GROUP BY i_brand_id, i_class_id, i_category_id
    HAVING SUM(cs_quantity * cs_list_price) > (SELECT average_sales FROM avg_sales)
    UNION ALL
    SELECT 'web' AS channel, i_brand_id, i_class_id, i_category_id,
           SUM(ws_quantity * ws_list_price) AS sales, COUNT(*) AS number_sales
    FROM web_sales
    JOIN item ON ws_item_sk = i_item_sk
    JOIN date_dim ON ws_sold_date_sk = d_date_sk
    WHERE ws_item_sk IN (SELECT ss_item_sk FROM cross_items)
      AND d_year = 1999 + 2
    GROUP BY i_brand_id, i_class_id, i_category_id
    HAVING SUM(ws_quantity * ws_list_price) > (SELECT average_sales FROM avg_sales)
) y
GROUP BY ROLLUP(channel, i_brand_id, i_class_id, i_category_id)
ORDER BY channel, i_brand_id, i_class_id, i_category_id
LIMIT 100;

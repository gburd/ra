-- TPC-DS Query 36
-- Compute gross profit ratio by store and item category
SELECT sum(ss_net_profit) / sum(ss_ext_sales_price) AS gross_margin,
       i_category,
       i_class,
       grouping(i_category) + grouping(i_class) AS lochierarchy,
       rank() OVER (
           PARTITION BY grouping(i_category) + grouping(i_class),
                        CASE WHEN grouping(i_class) = 0
                             THEN i_category END
           ORDER BY sum(ss_net_profit) / sum(ss_ext_sales_price) ASC
       ) AS rank_within_parent
FROM store_sales
JOIN date_dim d1 ON d1.d_date_sk = ss_sold_date_sk
JOIN item ON i_item_sk = ss_item_sk
JOIN store ON s_store_sk = ss_store_sk
WHERE d1.d_year = 2001
  AND s_state IN ('TN', 'SD', 'OH', 'NM', 'WI', 'AL', 'NC', 'OK')
GROUP BY ROLLUP(i_category, i_class)
ORDER BY lochierarchy DESC,
         CASE WHEN lochierarchy = 0 THEN i_category END,
         rank_within_parent
LIMIT 100;

-- TPC-DS Query 89: Revenue by specific items and categories for certain months
-- Identify items whose monthly revenue deviates significantly from average
SELECT *
FROM (
    SELECT
        i_category,
        i_class,
        i_brand,
        s_store_name,
        s_company_name,
        d_moy,
        SUM(ss_sales_price) AS sum_sales,
        AVG(SUM(ss_sales_price)) OVER (
            PARTITION BY i_category, i_brand, s_store_name, s_company_name
        ) AS avg_monthly_sales
    FROM item
    JOIN store_sales ON ss_item_sk = i_item_sk
    JOIN date_dim ON ss_sold_date_sk = d_date_sk
    JOIN store ON ss_store_sk = s_store_sk
    WHERE d_year = 1999
        AND (
            (i_category IN ('Books', 'Children', 'Electronics')
                AND i_class IN ('personal', 'portable', 'reference', 'self-help')
                AND i_brand IN (
                    'scholaramalgamalg #14',
                    'scholaramalgamalg #7',
                    'exportiunivamalg #9',
                    'scholaramalgamalg #9'
                ))
            OR
            (i_category IN ('Women', 'Music', 'Men')
                AND i_class IN ('accessories', 'classical', 'fragrances', 'pants')
                AND i_brand IN (
                    'amalgimporto #1',
                    'edu packscholar #1',
                    'exportiimporto #1',
                    'importoamalg #1'
                ))
        )
    GROUP BY
        i_category, i_class, i_brand,
        s_store_name, s_company_name, d_moy
) tmp1
WHERE CASE
    WHEN avg_monthly_sales <> 0
    THEN ABS(sum_sales - avg_monthly_sales) / avg_monthly_sales
    ELSE NULL
END > 0.1
ORDER BY sum_sales - avg_monthly_sales, s_store_name
LIMIT 100;

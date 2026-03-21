-- Year-over-year comparison
-- Shows window functions and time-series analysis

-- Compare monthly totals year-over-year
WITH monthly_summary AS (
    SELECT
        DATE_TRUNC('month', t.transaction_date) as month,
        a.account_type,
        SUM(t.debit_amount) as debit_total,
        SUM(t.credit_amount) as credit_total,
        COUNT(*) as transaction_count,
        COUNT(DISTINCT t.journal_entry_id) as journal_entries
    FROM ledger_transactions t
    JOIN chart_of_accounts a ON t.debit_account_code = a.account_code
    WHERE t.transaction_date >= DATE_TRUNC('year', CURRENT_DATE - INTERVAL '1 year')
    GROUP BY DATE_TRUNC('month', t.transaction_date), a.account_type
),
with_comparisons AS (
    SELECT
        month,
        account_type,
        debit_total,
        credit_total,
        transaction_count,

        -- Previous month
        LAG(debit_total, 1) OVER (
            PARTITION BY account_type
            ORDER BY month
        ) as prev_month_debits,

        -- Same month last year
        LAG(debit_total, 12) OVER (
            PARTITION BY account_type
            ORDER BY month
        ) as last_year_debits,

        -- Running total for the year
        SUM(debit_total) OVER (
            PARTITION BY account_type, EXTRACT(YEAR FROM month)
            ORDER BY month
            ROWS UNBOUNDED PRECEDING
        ) as ytd_debits,

        -- Rank within year
        RANK() OVER (
            PARTITION BY account_type, EXTRACT(YEAR FROM month)
            ORDER BY debit_total DESC
        ) as month_rank_in_year
    FROM monthly_summary
)
SELECT
    month,
    account_type,
    debit_total,
    credit_total,
    transaction_count,

    -- Month-over-month change
    ROUND(100.0 * (debit_total - prev_month_debits) / NULLIF(prev_month_debits, 0), 2) as mom_change_pct,

    -- Year-over-year change
    ROUND(100.0 * (debit_total - last_year_debits) / NULLIF(last_year_debits, 0), 2) as yoy_change_pct,

    ytd_debits,
    month_rank_in_year
FROM with_comparisons
WHERE month >= DATE_TRUNC('year', CURRENT_DATE)
ORDER BY account_type, month;

-- Optimization opportunities:
-- 1. Partition table by month for faster aggregation
-- 2. Create index on (transaction_date, account_type)
-- 3. Use parallel window function execution
-- 4. Consider materialized view for monthly summaries
-- Top accounts by activity
-- Shows ranking, filtering, and optimization techniques

-- Find top 10 most active accounts by transaction volume
WITH account_activity AS (
    SELECT
        account_code,
        SUM(transaction_count) as total_transactions,
        SUM(debit_amount) as total_amount,
        AVG(debit_amount) as avg_amount,
        MAX(last_transaction) as last_activity
    FROM (
        -- Debit side
        SELECT
            debit_account_code as account_code,
            COUNT(*) as transaction_count,
            SUM(debit_amount) as debit_amount,
            MAX(transaction_date) as last_transaction
        FROM ledger_transactions
        WHERE transaction_date >= CURRENT_DATE - INTERVAL '30 days'
        GROUP BY debit_account_code

        UNION ALL

        -- Credit side
        SELECT
            credit_account_code as account_code,
            COUNT(*) as transaction_count,
            SUM(credit_amount) as debit_amount,
            MAX(transaction_date) as last_transaction
        FROM ledger_transactions
        WHERE transaction_date >= CURRENT_DATE - INTERVAL '30 days'
        GROUP BY credit_account_code
    ) combined
    GROUP BY account_code
),
ranked_accounts AS (
    SELECT
        aa.account_code,
        a.account_name,
        a.account_type,
        aa.total_transactions,
        aa.total_amount,
        aa.avg_amount,
        aa.last_activity,
        ROW_NUMBER() OVER (ORDER BY aa.total_transactions DESC) as rank_by_count,
        ROW_NUMBER() OVER (ORDER BY aa.total_amount DESC) as rank_by_amount,
        DENSE_RANK() OVER (
            PARTITION BY a.account_type
            ORDER BY aa.total_transactions DESC
        ) as rank_within_type
    FROM account_activity aa
    JOIN chart_of_accounts a ON aa.account_code = a.account_code
)
SELECT
    rank_by_count,
    account_code,
    account_name,
    account_type,
    total_transactions,
    total_amount,
    ROUND(avg_amount, 2) as avg_amount,
    last_activity,
    rank_within_type
FROM ranked_accounts
WHERE rank_by_count <= 10
   OR rank_within_type = 1  -- Also include top account per type
ORDER BY rank_by_count;

-- Optimization opportunities:
-- 1. Covering index on (transaction_date, account_code, amount)
-- 2. Parallel UNION ALL execution
-- 3. Push date filter before aggregation
-- 4. Consider summary table for frequent queries
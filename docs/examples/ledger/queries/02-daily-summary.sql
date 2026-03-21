-- Daily transaction summary
-- Shows aggregation optimization and date-based filtering

-- Get daily transaction summary for current month
WITH daily_transactions AS (
    SELECT
        transaction_date,
        COUNT(*) as transaction_count,
        SUM(debit_amount) as total_debits,
        SUM(credit_amount) as total_credits,
        COUNT(DISTINCT debit_account_code) as unique_debit_accounts,
        COUNT(DISTINCT credit_account_code) as unique_credit_accounts
    FROM ledger_transactions
    WHERE transaction_date >= DATE_TRUNC('month', CURRENT_DATE)
      AND transaction_date < DATE_TRUNC('month', CURRENT_DATE) + INTERVAL '1 month'
    GROUP BY transaction_date
)
SELECT
    transaction_date,
    transaction_count,
    total_debits,
    total_credits,
    total_debits - total_credits as net_flow,
    unique_debit_accounts,
    unique_credit_accounts
FROM daily_transactions
ORDER BY transaction_date;

-- Optimization opportunities:
-- 1. Index on transaction_date for range scan
-- 2. Parallel aggregation for large datasets
-- 3. Consider materialized view for frequently accessed summaries
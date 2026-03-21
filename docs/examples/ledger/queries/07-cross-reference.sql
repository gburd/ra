-- Cross-reference analysis
-- Shows complex joins and correlation analysis

-- Find accounts that frequently transact together
WITH account_pairs AS (
    SELECT
        t.debit_account_code,
        t.credit_account_code,
        COUNT(*) as transaction_count,
        SUM(t.debit_amount) as total_amount,
        MIN(t.transaction_date) as first_transaction,
        MAX(t.transaction_date) as last_transaction,
        ARRAY_AGG(DISTINCT t.journal_entry_id ORDER BY t.journal_entry_id) as journal_entries
    FROM ledger_transactions t
    WHERE t.transaction_date >= CURRENT_DATE - INTERVAL '90 days'
    GROUP BY t.debit_account_code, t.credit_account_code
    HAVING COUNT(*) >= 5  -- At least 5 transactions
),
enriched_pairs AS (
    SELECT
        ap.debit_account_code,
        da.account_name as debit_account_name,
        da.account_type as debit_account_type,
        ap.credit_account_code,
        ca.account_name as credit_account_name,
        ca.account_type as credit_account_type,
        ap.transaction_count,
        ap.total_amount,
        ap.first_transaction,
        ap.last_transaction,
        ap.last_transaction - ap.first_transaction as relationship_duration,
        ap.transaction_count::float /
            GREATEST(EXTRACT(EPOCH FROM (ap.last_transaction - ap.first_transaction)) / 86400, 1) as transactions_per_day
    FROM account_pairs ap
    JOIN chart_of_accounts da ON ap.debit_account_code = da.account_code
    JOIN chart_of_accounts ca ON ap.credit_account_code = ca.account_code
)
SELECT
    debit_account_code,
    debit_account_name,
    debit_account_type,
    credit_account_code,
    credit_account_name,
    credit_account_type,
    transaction_count,
    total_amount,
    ROUND(total_amount / transaction_count, 2) as avg_transaction_amount,
    first_transaction,
    last_transaction,
    relationship_duration,
    ROUND(transactions_per_day, 2) as daily_frequency,
    CASE
        WHEN transactions_per_day > 1 THEN 'High Frequency'
        WHEN transactions_per_day > 0.5 THEN 'Medium Frequency'
        ELSE 'Low Frequency'
    END as frequency_category
FROM enriched_pairs
ORDER BY transaction_count DESC, total_amount DESC
LIMIT 20;

-- Optimization opportunities:
-- 1. Composite index on (debit_account_code, credit_account_code, transaction_date)
-- 2. Bitmap index scan for date range
-- 3. Hash join for account lookups
-- 4. Consider graph database for relationship analysis
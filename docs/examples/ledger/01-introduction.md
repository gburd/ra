# Chapter 1: Introduction to Query Optimization

## Meet Alice and Her Growing Business

Alice runs a small artisan coffee shop called "Bean Counter" in downtown Portland. Like any business owner, she needs to track every penny - from coffee bean purchases to daily sales, employee wages to equipment costs. She chose PostgreSQL for her accounting system, but as her business grows, she's starting to notice something: some queries that used to be instant now take seconds.

This is where our journey begins. Through Alice's eyes, we'll discover how database query optimizers work their magic, transforming slow queries into lightning-fast operations.

## What is Query Optimization?

When you write SQL, you're describing *what* you want, not *how* to get it. Query optimization is the process of figuring out the best *how*.

Consider Alice's simple question: "What's my current cash balance?"

```sql-interactive
-- The question is simple...
SELECT balance FROM accounts WHERE name = 'Cash';
```

But behind the scenes, the database has choices:
- Scan every row in the table?
- Use an index if one exists?
- Check statistics to estimate how many rows match?

## The RA Optimizer

RA is a pedagogical query optimizer - it's designed to teach by showing. Unlike production optimizers that hide their reasoning, RA exposes every decision:

```sql-interactive
-- Let's see RA in action
EXPLAIN SELECT
    account_name,
    SUM(debit_amount - credit_amount) as balance
FROM ledger_transactions
WHERE account_code = '1010'
GROUP BY account_name;
```

### What RA Shows You

1. **Logical Plan**: The abstract operations needed
2. **Physical Plan**: The concrete execution strategy
3. **Cost Estimates**: Why one plan beats another
4. **Rule Applications**: Which optimizations fired
5. **Statistics Used**: What data influenced decisions

## Alice's Accounting Schema

Before we dive deeper, let's understand Alice's database structure:

### Core Tables

1. **chart_of_accounts**: The account hierarchy
   - Assets (1000s): Cash, Inventory, Equipment
   - Liabilities (2000s): Loans, Accounts Payable
   - Equity (3000s): Owner's Capital
   - Revenue (4000s): Sales, Services
   - Expenses (5000s): Supplies, Rent, Wages

2. **ledger_transactions**: Every financial movement
   - Double-entry: Every debit has a credit
   - Multi-currency: USD, EUR, supplier credits
   - Audit trail: Who, what, when

3. **journal_entries**: Transaction groups
   - Daily sales batches
   - Monthly payroll runs
   - Inventory purchases

## Your First Optimization

Let's help Alice with her morning routine - checking yesterday's sales:

```sql-interactive
-- Version 1: The naive approach
SELECT
    je.entry_date,
    je.description,
    SUM(lt.credit_amount) as total_sales
FROM journal_entries je
JOIN ledger_transactions lt ON je.id = lt.journal_entry_id
WHERE je.entry_date = CURRENT_DATE - 1
  AND lt.credit_account_code = '4010'  -- Sales Revenue
GROUP BY je.entry_date, je.description;
```

Now watch RA optimize this:

```sql-interactive
-- Same query with optimization hints
SELECT /*+ USE_INDEX(je date_idx) */
    je.entry_date,
    je.description,
    SUM(lt.credit_amount) as total_sales
FROM journal_entries je
JOIN ledger_transactions lt ON je.id = lt.journal_entry_id
WHERE je.entry_date = CURRENT_DATE - 1
  AND lt.credit_account_code = '4010'
GROUP BY je.entry_date, je.description;
```

## Interactive Elements

###  Try It Yourself

Modify the query above to:
1. Look at last week instead of yesterday
2. Include both sales (4010) and service revenue (4020)
3. Add the entry_number to the output

###  Statistics Impact

```statistics-editor
{
  "journal_entries": {
    "row_count": 10000,
    "entry_date_cardinality": 365,
    "has_date_index": true
  },
  "ledger_transactions": {
    "row_count": 50000,
    "account_cardinality": 150,
    "has_account_index": false
  }
}
```

Toggle the `has_account_index` to see how the plan changes!

###  Facts Configuration

```facts-editor
{
  "rules": {
    "PushFilterThroughJoin": true,
    "UseIndexSeek": true,
    "MergeJoinOnSorted": false,
    "HashJoinForLarge": true
  }
}
```

Disable `PushFilterThroughJoin` and observe the cost increase.

## Key Takeaways

1. **Query optimization transforms *what* into *how***
   - SQL describes the result
   - The optimizer finds the best path

2. **Multiple valid plans exist**
   - Each has different costs
   - The "best" depends on your data

3. **Statistics matter**
   - Row counts influence join order
   - Cardinality affects index usage
   - Selectivity drives filter placement

4. **Rules are building blocks**
   - Each rule is a specific optimization
   - Rules combine for complex improvements
   - Not all rules apply to every query

## Coffee Break Challenge

Alice notices her month-end reports are slow. Can you identify why?

```sql-interactive
-- The slow month-end report
SELECT
    a.account_type,
    a.account_name,
    SUM(
        CASE
            WHEN t.debit_account_code = a.account_code
            THEN t.debit_amount
            WHEN t.credit_account_code = a.account_code
            THEN -t.credit_amount
            ELSE 0
        END
    ) as month_total
FROM chart_of_accounts a
CROSS JOIN ledger_transactions t
WHERE t.transaction_date >= DATE_TRUNC('month', CURRENT_DATE)
  AND t.transaction_date < DATE_TRUNC('month', CURRENT_DATE) + INTERVAL '1 month'
GROUP BY a.account_type, a.account_name
ORDER BY a.account_type, a.account_name;
```

*Hint: Look at the join type and the WHERE clause relationship...*

## Next Steps

Now that you understand the basics, let's explore Alice's schema in detail. In [Chapter 2: Schema Design](02-schema.md), we'll see how table structure influences optimization decisions.

### What's Coming

- **Chapter 2**: Deep dive into the schema
- **Chapter 3**: Basic query patterns and their optimizations
- **Chapter 4**: Aggregations and grouping strategies

---

* Remember: The goal isn't to memorize rules, but to understand the reasoning. RA shows you the "why" behind every optimization decision.*
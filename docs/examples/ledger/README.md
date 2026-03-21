# RA Query Optimizer: An Interactive Ledger Guide

Welcome to the interactive guide for RA, a pedagogical query optimizer that helps you understand how databases transform SQL queries into efficient execution plans. Through the story of Alice's growing accounting business, you'll learn how query optimization works from the ground up.

## 🎯 What You'll Learn

- **Query Planning**: How SQL queries become execution plans
- **Cost Models**: Why databases choose one plan over another
- **Statistics**: How data distribution affects optimization
- **Rules & Transformations**: The building blocks of optimization
- **Hardware Awareness**: How CPU and memory influence plans
- **Dialect Translation**: Adapting queries for different databases

## 📚 Guide Structure

### Part 1: Getting Started
1. [**Introduction**](01-introduction.md) - Meet Alice and her ledger system
2. [**Schema Design**](02-schema.md) - Understanding the database structure
3. [**Basic Queries**](03-basic-queries.md) - Your first optimizations

### Part 2: Growing Complexity
4. [**Aggregations**](04-aggregations.md) - Summarizing financial data
5. [**Window Functions**](05-window-functions.md) - Running totals and rankings
6. [**Optimization Journey**](06-optimization-journey.md) - Watch RA optimize step-by-step

### Part 3: Advanced Features
7. [**Statistics Impact**](07-statistics-impact.md) - How data shapes decisions
8. [**Dialect Translation**](08-dialect-translation.md) - One query, many databases
9. [**Hardware Awareness**](09-hardware-awareness.md) - Adapting to your machine
10. [**Advanced Features**](10-advanced-features.md) - Covering indexes, bitmap scans, and more

### Interactive Lab
- [**Query Optimization Lab**](interactive.md) - Hands-on WASM-powered query editor with adjustable statistics and index controls

## 🚀 Quick Start

Each section includes interactive SQL examples. Try modifying them to see how RA responds:

```sql-interactive
-- Alice's first query: What's my cash balance?
SELECT
    a.account_name,
    SUM(CASE
        WHEN t.debit_account_code = a.account_code
        THEN t.debit_amount
        ELSE -t.credit_amount
    END) as balance
FROM chart_of_accounts a
LEFT JOIN ledger_transactions t ON
    t.debit_account_code = a.account_code OR
    t.credit_account_code = a.account_code
WHERE a.account_code = '1010'  -- Cash account
GROUP BY a.account_code, a.account_name;
```

## 🎮 Interactive Features

### Statistics Editor
Adjust table statistics and watch plans change in real-time:
- Table row counts
- Column cardinalities
- Index availability

### Facts Configuration
Control which optimization rules apply:
- Toggle facts (hasIndex, isNotNull, etc.)
- See precondition matching
- Understand rule firing

### Plan Visualization
- Tree visualization of query plans
- Side-by-side before/after comparisons
- Cost breakdown charts
- Optimization timeline

## 📖 The Story

Follow Alice as her business grows from a small shop to a thriving enterprise:

1. **Day 1**: Simple balance lookups
2. **Month 1**: Monthly reports need aggregations
3. **Year 1**: Performance issues emerge
4. **Year 2**: Indexes save the day
5. **Year 3**: Multi-currency challenges
6. **Today**: Enterprise-scale optimization

## 🛠️ How to Use This Guide

### For Learning
- Start at the beginning and follow Alice's journey
- Try the interactive examples
- Experiment with statistics and facts
- Compare different optimization strategies

### For Reference
- Jump to specific topics as needed
- Use the query library in `/queries`
- Check optimization patterns
- Review cost model calculations

## 🔬 Under the Hood

RA demonstrates real optimization techniques:
- **Cascades Framework**: Top-down cost-based optimization
- **Transformation Rules**: Pattern matching and rewriting
- **Cost Estimation**: Cardinality and selectivity calculations
- **Physical Properties**: Ordering, distribution, and indexing

## 💡 Key Concepts

### Query Plans
- **Logical Plans**: What to compute
- **Physical Plans**: How to compute it
- **Cost Models**: Choosing the best approach

### Optimization Rules
- **Transformation**: Equivalent query rewrites
- **Implementation**: Logical to physical mapping
- **Pruning**: Eliminating inferior plans

### Statistics & Costs
- **Cardinality**: Row count estimates
- **Selectivity**: Filter effectiveness
- **Cost Units**: CPU, I/O, memory, network

## 🎯 Learning Objectives

By the end of this guide, you'll understand:

1. ✅ How query optimizers make decisions
2. ✅ Why indexes dramatically improve performance
3. ✅ How statistics influence plan selection
4. ✅ When rules fire and why they matter
5. ✅ How to read and interpret query plans
6. ✅ Cost model calculations and tradeoffs
7. ✅ Hardware's role in optimization
8. ✅ Cross-database query translation

## 📝 Prerequisites

- Basic SQL knowledge
- Familiarity with database concepts
- No optimization experience required!

## 🚦 Ready to Start?

Begin your journey with [Chapter 1: Introduction](01-introduction.md) →

---

*This guide is part of the RA Query Optimizer project. RA is designed for education and experimentation, helping developers understand the magic behind database query optimization.*
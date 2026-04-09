# Visualization Guide

RA Web provides five visualization modes to help you understand and optimize query execution plans.

## Overview

Each output panel contains five tabs:

1. **Raw Plan** - Text view with syntax highlighting
2. **Tree View** - Hierarchical tree diagram
3. **Flow View** - Data flow visualization
4. **Cost Analysis** - Performance metrics and charts
5. **Warnings** - Detected optimization issues

Switch between tabs to analyze different aspects of the query plan.

## Raw Plan View

### What It Shows

The raw query plan as returned by the database, enhanced with:

- Syntax highlighting (operations, keywords, metrics)
- Collapsible tree nodes
- In-text search with match highlighting
- Cost/time badges on relevant lines

### When to Use

- Examining specific operation details
- Finding text patterns (table names, column names, conditions)
- Comparing raw output across engines
- Debugging unexpected behavior

### Features

**Syntax Highlighting**

Operations like `Seq Scan`, `Hash Join`, `Index Scan` appear in cyan. Keywords like `SELECT`, `WHERE` appear in blue. Metrics like `cost`, `rows`, `width` appear in green.

**Collapsible Nodes**

Click the collapse icon (▼/▶) next to operations to hide/show child operations. Useful for focusing on specific parts of large plans.

**Search**

Click the search icon (magnifying glass) to open the search bar:

- Type search term
- Navigate matches with Next/Previous arrows
- Current match highlighted in orange
- Other matches highlighted in gold
- Press Escape to close search

**Expand/Collapse All**

Use the control buttons above the plan to expand or collapse all nodes at once.

### Interpreting the Output

Example PostgreSQL plan:

```
Nested Loop  (cost=0.29..16.76 rows=10 width=68)
  ->  Seq Scan on departments d  (cost=0.00..1.15 rows=10 width=36)
  ->  Index Scan using employees_pkey on employees e  (cost=0.29..1.55 rows=1 width=32)
        Index Cond: (department_id = d.id)
```

Key elements:

- **Operation** - `Seq Scan`, `Index Scan`, `Nested Loop`
- **Cost** - `0.29..16.76` (startup..total estimated cost)
- **Rows** - Estimated number of rows returned
- **Width** - Average row size in bytes
- **Indentation** - Shows parent-child relationships

When ANALYZE is enabled, you also see:

```
Nested Loop  (cost=0.29..16.76 rows=10 width=68) (actual time=0.123..0.456 rows=8 loops=1)
```

- **actual time** - Real startup and total execution time (milliseconds)
- **rows** - Actual number of rows (compare to estimate)
- **loops** - Number of times operation repeated

## Tree View

### What It Shows

A hierarchical visualization of the query plan as an interactive tree diagram.

### When to Use

- Understanding plan structure at a glance
- Identifying join orders and nesting depth
- Spotting unusual patterns (deep nesting, unbalanced joins)
- Teaching query optimization concepts

### Features

**Interactive Nodes**

- Hover over nodes for detailed tooltips
- Click to highlight (synchronized across panels)
- Color-coded by operation type:
  - Blue: Scan operations
  - Green: Join operations
  - Orange: Aggregate operations
  - Purple: Sort operations
  - Gray: Other operations

**Zoom and Pan**

- Mouse wheel to zoom in/out
- Click and drag to pan
- Reset button to restore default view

**Cost Indicators**

Node size represents relative cost. Larger nodes = more expensive operations.

### Interpreting the Visualization

Example tree structure:

```
        Hash Join
       /         \
   Seq Scan    Hash
              /
         Index Scan
```

This shows:
- A hash join operation at the root
- Left input: Sequential scan
- Right input: Hash operation (fed by an index scan)

The tree makes join order explicit: the system scans tables, builds a hash table, then performs the join.

**Common Patterns**

*Balanced tree* - Good join order, similar depths on both sides

*Left-deep tree* - Series of joins with one input always a table scan (common in OLTP)

*Right-deep tree* - Hash builds on the right side (less common)

*Bushy tree* - Multiple joins at the same level (parallel execution possible)

### Tips

- Deep nesting (>5 levels) may indicate complex query or suboptimal plan
- Asymmetric trees suggest optimizer chose one table as "driving" table
- Multiple nodes with same name may indicate CTE reuse

## Flow View

### What It Shows

A data flow diagram showing how rows move through the query execution pipeline.

### When to Use

- Understanding data transformations
- Identifying bottlenecks (narrow pipes = few rows, thick pipes = many rows)
- Tracing row count reduction through filters
- Visualizing parallelism and data exchange

### Features

**Arrows with Width**

Arrow thickness represents estimated row count. Follow the flow to see where data is filtered or expanded.

**Color-Coded Operations**

Same color scheme as Tree View, but arranged left-to-right:

- Data flows from left (scans) to right (output)
- Vertical arrangement shows parallel streams
- Converging arrows show joins/unions

**Cardinality Labels**

Numbers on arrows show estimated row counts at each stage.

### Interpreting the Visualization

Example flow:

```
[employees]──(10,000)──>[Filter]──(100)──>\
                                           [Join]──(150)──>[Output]
[departments]──(10)──>[Filter]──(5)─────>/
```

This shows:
1. Scan employees (10,000 rows)
2. Apply filter (reduces to 100 rows)
3. Scan departments (10 rows)
4. Apply filter (reduces to 5 rows)
5. Join (produces 150 rows - join expansion)
6. Return result

**Key Insights**

- Large drops in cardinality = effective filters (good)
- Small drops = ineffective filters (might be unnecessary)
- Increases = join explosion or cartesian product (investigate)
- Parallel flows = can be executed concurrently

### Tips

- Thick arrows early in the pipeline indicate missing indexes
- Join expansions (output > both inputs) suggest missing join predicates
- Bottlenecks often occur at joins or sorts

## Cost Analysis View

### What It Shows

Performance metrics and visual comparisons between estimated and actual costs.

### When to Use

- Identifying estimation errors (actual >> estimated)
- Comparing operation costs
- Finding expensive operations
- Validating index usage
- Tuning query performance

### Features

**Cost Breakdown Table**

Tabular view of all operations with columns:

- **Operation** - Name and type
- **Estimated Cost** - Optimizer's prediction
- **Actual Cost** - Real execution time (ANALYZE only)
- **Rows (Est)** - Estimated row count
- **Rows (Act)** - Actual row count (ANALYZE only)
- **Variance** - Percentage difference between estimate and actual

**Bar Charts**

Horizontal bars showing relative costs:

- Blue bar: Estimated cost
- Orange bar: Actual cost (ANALYZE mode)
- Variance indicator when estimate is far from actual

**Cost Distribution**

Pie chart showing which operations consume the most time/resources.

### Interpreting the Output

Example cost table:

| Operation     | Est Cost | Act Cost | Rows (Est) | Rows (Act) | Variance |
|---------------|----------|----------|------------|------------|----------|
| Seq Scan      | 145.00   | 23.45    | 10000      | 10000      | -84%     |
| Index Scan    | 8.50     | 234.67   | 100        | 8500       | +2660%   |
| Hash Join     | 432.10   | 412.98   | 1500       | 1520       | -4%      |

**Red Flags**

1. **Large positive variance** (actual >> estimated)
   - Optimizer underestimated cost
   - May indicate missing statistics (run ANALYZE)
   - Could suggest better index or rewrite

2. **Large negative variance** (actual << estimated)
   - Optimizer overestimated cost
   - Plan is better than expected (good!)
   - May have chosen suboptimal plan due to overestimation

3. **Row count mismatches**
   - Indicates cardinality estimation errors
   - Often caused by correlated columns or skewed data
   - May need extended statistics or query hints

### Tips

- Focus on operations with >10% of total cost
- Variances >100% indicate serious estimation problems
- Sequential scans aren't always bad (small tables are fast)
- Index scans can be slow if selectivity is poor

## Warnings View

### What It Shows

Automatically detected query optimization issues and recommendations.

### When to Use

- Quick health check for new queries
- Identifying common antipatterns
- Learning optimization techniques
- Prioritizing optimization efforts

### Features

**Severity Levels**

- **Critical** (red) - Major performance impact, fix immediately
- **Warning** (orange) - Moderate impact, should investigate
- **Info** (blue) - Optimization opportunity, consider improving

**Categories**

1. **Missing Indexes** - Sequential scans that would benefit from indexes
2. **Inefficient Joins** - Nested loops with high cardinality, cartesian products
3. **Estimation Errors** - Cardinality mismatches >10x
4. **Expensive Operations** - Sorts, hash operations with large row counts
5. **Suboptimal Plans** - Known antipatterns (filesort, temp tables)

**Recommendations**

Each warning includes:
- Description of the issue
- Affected operation(s)
- Suggested fix
- Estimated impact

### Common Warnings

**Missing Index**

```
⚠ Warning: Sequential Scan on large table
Table: orders (10,000,000 rows)
Filter: WHERE customer_id = 123
Recommendation: CREATE INDEX idx_orders_customer_id ON orders(customer_id);
Impact: 95% reduction in query time
```

**Cartesian Product**

```
🔴 Critical: Cartesian product detected
Tables: orders × line_items
Rows: 1,000,000 × 5,000,000 = 5,000,000,000,000
Recommendation: Add join condition: orders.id = line_items.order_id
```

**Cardinality Mismatch**

```
⚠ Warning: Row estimate off by 1000x
Operation: Index Scan
Estimated: 10 rows
Actual: 10,000 rows
Recommendation: Run ANALYZE orders; or use extended statistics
```

**Expensive Sort**

```
⚠ Warning: Sort operation on 1M rows
Operation: Sort
Memory: 128MB estimated
Recommendation: Add index on sorted columns or use LIMIT
```

### Interpreting Recommendations

Not all warnings require action:

**Act immediately:**
- Cartesian products (missing join conditions)
- Sequential scans on multi-million row tables with filters
- Sorts using excessive memory

**Investigate:**
- Cardinality mismatches >100x
- Nested loops with high iteration counts
- Excessive temporary tables

**Consider:**
- Minor estimation errors (<10x)
- Small table sequential scans (faster than index)
- Informational tips

### Tips

- Fix critical issues first (biggest impact)
- Run ANALYZE after schema changes
- Some warnings are false positives (small tables, cached data)
- Test before/after to measure improvement

## Cross-Panel Synchronization

When comparing multiple engines side-by-side:

**Synchronized Highlighting**

Hover over or click a node in Tree View or Flow View - the corresponding node highlights in all panels. This helps identify:

- Same operations across engines
- Different join orders
- Plan structure differences

**Coordinated Navigation**

Search terms and zoom levels can be synchronized (optional feature) to compare exact positions.

## Tips for All Visualizations

1. **Start with Raw Plan** - Understand the basic structure
2. **Use Tree View** - Get spatial intuition for plan shape
3. **Check Flow View** - Verify cardinalities make sense
4. **Review Cost Analysis** - Find expensive operations
5. **Act on Warnings** - Address critical issues first

Different visualizations reveal different insights. Use multiple views to build a complete picture of query performance.

## Screenshot Placeholders

*Note: Screenshots will be added in a future update*

- Raw Plan with search highlighting
- Tree View with interactive nodes
- Flow View showing data flow
- Cost Analysis bar charts
- Warnings View with recommendations
- Multi-panel comparison with synchronized highlighting

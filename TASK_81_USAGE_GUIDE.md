# Task #81: Interactive Plan Visualization - Usage Guide

## Quick Start

### 1. Launch the Application

```bash
cd /home/gburd/ws/ra
cargo run -p ra-web
```

The server will start on `http://localhost:8000`

### 2. Access the Demo

Navigate to: `http://localhost:8000/plan-visualization.html`

Or click the "Interactive Plan Visualization" card from the main demo page.

## User Interface Overview

```
┌───────────────────────────────────────────────────────────────┐
│  Interactive Query Plan Visualization                         │
│  Visualize and compare query plans with interactive cost      │
│  analysis                                                      │
└───────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  Control Panel                                               │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  SQL Query:                                                  │
│  ┌────────────────────────────────────────────────────────┐ │
│  │ SELECT u.name, COUNT(o.id) as order_count             │ │
│  │ FROM users u                                           │ │
│  │ JOIN orders o ON u.id = o.user_id                     │ │
│  │ WHERE u.age > 25 AND o.total > 100                    │ │
│  │ GROUP BY u.id, u.name                                 │ │
│  │ ORDER BY order_count DESC                             │ │
│  │ LIMIT 10                                              │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
│  Hardware Profile:  [Auto Detect ▼]                         │
│                                                              │
│  [Visualize Plan]  [Compare Plans]                          │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Single Plan View Mode

### What You'll See

```
┌─────────────────────────────────────────────────────────────┐
│  Statistics Summary                                          │
├──────────────┬──────────────┬──────────────┬───────────────┤
│ Total Cost   │ Rules Applied│ Plan Nodes   │ Hardware      │
│    1247.50   │      12      │     15       │ Auto-Detect   │
└──────────────┴──────────────┴──────────────┴───────────────┘

┌─────────────────────────────────────────────────────────────┐
│  Query Plan                          Total Cost: 1247.50    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│                    ┌─────────────┐                          │
│                    │   Limit     │  ← Root node             │
│                    │  Cost: 5.0  │                          │
│                    │  Rows: 10   │                          │
│                    └──────┬──────┘                          │
│                           │                                  │
│                    ┌──────▼──────┐                          │
│                    │    Sort     │                          │
│                    │ Cost: 150.0 │                          │
│                    │ Rows: 2500  │                          │
│                    └──────┬──────┘                          │
│                           │                                  │
│                    ┌──────▼──────┐                          │
│                    │HashAggregate│                          │
│                    │ Cost: 200.0 │                          │
│                    │ Rows: 250   │                          │
│                    └──────┬──────┘                          │
│                           │                                  │
│                    ┌──────▼──────┐                          │
│                    │  HashJoin   │                          │
│                    │ Cost: 300.0 │                          │
│                    │ Rows: 2500  │                          │
│                    └──┬──────┬───┘                          │
│                       │      │                               │
│              ┌────────▼──┐ ┌▼────────┐                     │
│              │  Filter   │ │ SeqScan │                     │
│              │ Cost: 50  │ │Cost: 100│                     │
│              │ Rows: 2500│ │Rows: 500│                     │
│              └─────┬─────┘ └─────────┘                     │
│                    │                                         │
│              ┌─────▼─────┐                                 │
│              │  SeqScan  │                                 │
│              │ Cost: 100 │                                 │
│              │Rows: 10000│                                 │
│              └───────────┘                                 │
│                                                              │
│  Color Legend:                                              │
│  ■ Blue=Scan  ■ Yellow=Filter  ■ Red=Join                  │
│  ■ Green=Aggregate  ■ Purple=Sort                          │
│                                                              │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  Cost Breakdown by Component                                 │
├─────────────────────────────────────────────────────────────┤
│  CPU        I/O        Memory     Network                    │
│  ┌───┐     ┌───┐     ┌───┐      ┌───┐                      │
│  │▓▓▓│     │▓▓▓│     │▓▓▓│      │   │                      │
│  │▓▓▓│     │▓▓▓│     │▓▓▓│      │   │                      │
│  │▓▓▓│     │▓▓▓│     │▓▓▓│      │   │                      │
│  │▓▓▓│     │▓▓▓│     │▓▓▓│      │   │                      │
│  └───┘     └───┘     └───┘      └───┘                      │
│  498.0     374.2     374.2       0.0                        │
└─────────────────────────────────────────────────────────────┘

[Expand All]  [Collapse All]  [Reset Zoom]
```

### Interactive Features

#### 1. Click to Collapse/Expand

```
Before Click:                  After Click:
┌──────────┐                   ┌──────────┐
│ HashJoin │                   │ HashJoin │
└─────┬────┘                   └──────────┘
      │                        (children hidden)
  ┌───┴───┐
  │       │
Filter  SeqScan
```

#### 2. Hover for Tooltips

```
When hovering over "HashJoin" node:

┌─────────────────────────────┐
│ HashJoin                    │
├─────────────────────────────┤
│ Cost: 300.00                │
│ Rows: 2,500                 │
│ Join Type: Inner            │
│ Condition: u.id = o.user_id │
└─────────────────────────────┘
```

#### 3. Zoom and Pan

- **Scroll**: Zoom in/out
- **Click + Drag**: Pan around large plans
- **Reset Zoom button**: Return to default view

## Comparison Mode

### What You'll See

```
┌─────────────────────────────────────────────────────────────┐
│  Statistics Summary                                          │
├──────────────┬──────────────┬──────────────┬───────────────┤
│ Original Cost│Optimized Cost│ Improvement  │ Best Optimizer│
│   1500.00    │   1247.50    │   16.8%      │     Ra        │
└──────────────┴──────────────┴──────────────┴───────────────┘

┌──────────────────────────────┬──────────────────────────────┐
│  Original Plan               │  Optimized Plan              │
│  Cost: 1500.00               │  Cost: 1247.50               │
├──────────────────────────────┼──────────────────────────────┤
│                              │                              │
│    ┌──────────┐              │    ┌──────────┐             │
│    │  Limit   │              │    │  Limit   │             │
│    └────┬─────┘              │    └────┬─────┘             │
│         │                    │         │                    │
│    ┌────▼─────┐              │    ┌────▼─────┐             │
│    │NestedLoop│ ← Expensive  │    │HashJoin  │ ← Optimized│
│    │Cost: 800 │              │    │Cost: 300 │             │
│    └────┬─────┘              │    └────┬─────┘             │
│         │                    │         │                    │
│    [Rest of tree...]         │    [Rest of tree...]        │
│                              │                              │
└──────────────────────────────┴──────────────────────────────┘

[Expand All]  [Collapse All]
```

### Cost Comparison

The comparison mode highlights:
- **Green badge**: Better (optimized) cost
- **Red badge**: Worse (original) cost
- **Percentage improvement**: Shows optimization gains

## Example Queries

### 1. Simple Select with Filter

```sql
SELECT * FROM users WHERE age > 25;
```

**Expected Plan**:
```
Filter (age > 25)
  └─ SeqScan (users)
```

**Use Case**: Understanding basic predicate pushdown

### 2. Join Query

```sql
SELECT u.name, o.total
FROM users u
JOIN orders o ON u.id = o.user_id;
```

**Expected Plan**:
```
HashJoin (u.id = o.user_id)
  ├─ SeqScan (users)
  └─ SeqScan (orders)
```

**Use Case**: Comparing join algorithms (hash vs nested loop)

### 3. Aggregation Query

```sql
SELECT department, COUNT(*), AVG(salary)
FROM employees
GROUP BY department;
```

**Expected Plan**:
```
HashAggregate (GROUP BY department)
  └─ SeqScan (employees)
```

**Use Case**: Understanding aggregation strategies

### 4. Complex Query (Pre-filled Example)

```sql
SELECT u.name, COUNT(o.id) as order_count
FROM users u
JOIN orders o ON u.id = o.user_id
WHERE u.age > 25 AND o.total > 100
GROUP BY u.id, u.name
ORDER BY order_count DESC
LIMIT 10;
```

**Expected Plan**:
```
Limit (10)
  └─ Sort (order_count DESC)
      └─ HashAggregate (GROUP BY u.id, u.name)
          └─ HashJoin (u.id = o.user_id)
              ├─ Filter (u.age > 25)
              │   └─ SeqScan (users)
              └─ Filter (o.total > 100)
                  └─ SeqScan (orders)
```

**Use Case**: Comprehensive optimization demonstration

### 5. Subquery Unnesting

```sql
SELECT * FROM users u
WHERE u.id IN (SELECT user_id FROM orders WHERE total > 100);
```

**Expected Plan (Before Optimization)**:
```
Filter (u.id IN subquery)
  ├─ SeqScan (users)
  └─ SeqScan (orders)
```

**Expected Plan (After Optimization)**:
```
HashJoin (u.id = orders.user_id)
  ├─ SeqScan (users)
  └─ Filter (total > 100)
      └─ SeqScan (orders)
```

**Use Case**: Demonstrating subquery transformation

## Hardware Profile Selection

### Available Profiles

1. **Auto Detect** (default)
   - Uses detected hardware capabilities
   - Best for local development

2. **GPU Server**
   - Simulates high-end GPU acceleration
   - Shows GPU-accelerated operators
   - Higher speedup for scan/join operations

3. **FPGA Appliance**
   - Simulates FPGA hardware
   - Shows streaming filter optimizations
   - Better for pipeline operations

4. **Standard Laptop**
   - Simulates typical laptop hardware
   - CPU-only execution
   - Realistic for most users

### How Hardware Affects Plans

```
Query: SELECT * FROM large_table WHERE value > 1000

CPU Only:
  Filter
    └─ SeqScan (CPU)

GPU Server:
  Filter (GPU-accelerated)
    └─ ParallelScan (GPU)
        Speedup: 5x
```

## Cost Interpretation

### Understanding Cost Values

Cost is a **dimensionless unit** representing the estimated computational expense:

- **Low Cost** (< 100): Simple operations, small data
- **Medium Cost** (100 - 1000): Moderate complexity
- **High Cost** (> 1000): Complex operations, large data

### Cost Components

```
Total Cost = CPU + I/O + Memory + Network

Example Breakdown:
┌──────────┬─────────┬──────────┐
│ CPU      │ I/O     │ Memory   │
│ 40%      │ 35%     │ 25%      │
│ (compute)│ (disk)  │ (buffers)│
└──────────┴─────────┴──────────┘
```

### Operator-Specific Costs

| Operator     | Cost Formula                    | Typical Range |
|--------------|---------------------------------|---------------|
| SeqScan      | rows × 0.01                     | 100 - 10,000  |
| IndexScan    | rows × 0.001                    | 15 - 500      |
| Filter       | input_cost + (rows × 0.001)     | 50 - 5,000    |
| HashJoin     | (left + right) × 0.1            | 300 - 50,000  |
| NestedLoop   | left × right × 0.01             | 1000 - 1M     |
| HashAggregate| input × 2.0                     | 200 - 20,000  |
| Sort         | input × 3.0                     | 150 - 30,000  |

## Troubleshooting

### Common Issues

#### 1. Empty Visualization

**Problem**: No plan appears after clicking "Visualize Plan"

**Solutions**:
- Check browser console for errors (F12)
- Verify SQL syntax is valid
- Ensure server is running on port 8000
- Check network tab for failed API requests

#### 2. Parse Errors

**Problem**: "Failed to parse SQL" error message

**Solutions**:
- Verify SQL syntax (use standard SQL)
- Check for typos in table/column names
- Ensure proper quote usage (' for strings)
- Try a simpler query first

#### 3. Large Plans Don't Fit

**Problem**: Plan tree is too large to see

**Solutions**:
- Use **Collapse All** to hide details
- **Zoom out** using scroll wheel
- **Pan** by click-dragging
- Click **Reset Zoom** to recenter

#### 4. Tooltip Doesn't Show

**Problem**: Hovering doesn't display details

**Solutions**:
- Ensure mouse is directly over node rectangle
- Check if tooltip is behind other elements
- Reload page if tooltips stop working

## Best Practices

### 1. Start Simple

Begin with basic queries and progressively add complexity:
```sql
-- Start here
SELECT * FROM users;

-- Then add filters
SELECT * FROM users WHERE age > 25;

-- Then joins
SELECT * FROM users u JOIN orders o ON u.id = o.user_id;

-- Finally, full complexity
SELECT u.name, COUNT(*) FROM users u
JOIN orders o ON u.id = o.user_id
WHERE u.age > 25
GROUP BY u.name;
```

### 2. Use Comparison Mode for Learning

Compare how different optimizers handle the same query:
- Identify optimization opportunities
- Understand algorithm tradeoffs
- Learn database-specific behaviors

### 3. Collapse Deep Trees

For queries with many levels:
1. Click root node to collapse
2. Expand only branches of interest
3. Focus on specific optimization areas

### 4. Experiment with Hardware Profiles

Try the same query with different profiles:
1. Note cost changes
2. Observe operator selection
3. Understand hardware impact

## Advanced Features

### 1. Reading Complex Plans

```
┌─────────────────────────────────────┐
│  Limit (rows: 10)                   │  ← Final output limit
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  Sort (cost: 150)                   │  ← Expensive sort
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  HashAggregate (cost: 200)          │  ← Grouping
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  HashJoin (cost: 300)               │  ← Main join
└────────┬─────────────┬──────────────┘
         │             │
    Left Input    Right Input

Reading Order: Bottom-up
  1. Scan base tables (leaves)
  2. Apply filters
  3. Join tables
  4. Aggregate results
  5. Sort output
  6. Limit rows
```

### 2. Identifying Optimization Opportunities

Look for these patterns:

**High-Cost Nested Loops**:
```
NestedLoop (cost: 50,000) ← Replace with HashJoin
  ├─ SeqScan (rows: 10,000)
  └─ SeqScan (rows: 5,000)
```

**Missing Filters**:
```
Join                        Better:
  ├─ SeqScan (10K rows)    Join
  └─ SeqScan (10K rows)      ├─ Filter → SeqScan (2K rows)
                              └─ Filter → SeqScan (1K rows)
```

**Unnecessary Sorts**:
```
Sort (after indexed scan)  ← Remove if index provides order
  └─ IndexScan (ordered)
```

### 3. Performance Metrics

Track these key metrics:
- **Total Cost**: Overall query expense
- **Node Count**: Plan complexity
- **Join Strategy**: Hash vs Nested Loop
- **Scan Method**: Sequential vs Index
- **Cost Distribution**: CPU vs I/O vs Memory

## Tips and Tricks

### Keyboard Shortcuts (Future Enhancement)

While not yet implemented, consider these patterns:
- Space: Toggle collapse on selected node
- Arrow keys: Navigate between nodes
- +/-: Zoom in/out
- R: Reset zoom

### URL Sharing (Future Enhancement)

Save and share plans:
```
http://localhost:8000/plan-visualization.html?sql=<encoded>
```

### Export Options (Future Enhancement)

Export visualizations as:
- PNG image
- SVG vector
- JSON data
- CSV cost breakdown

## Learning Resources

### Understanding Query Plans

1. **Operator Semantics**:
   - Scan: Read table rows
   - Filter: Apply WHERE conditions
   - Join: Combine tables
   - Aggregate: GROUP BY operations
   - Sort: ORDER BY operations

2. **Cost Model**:
   - Based on cardinality estimates
   - Considers I/O, CPU, memory
   - Hardware-aware adjustments

3. **Optimization Rules**:
   - Predicate pushdown
   - Join reordering
   - Projection pruning
   - Index selection

### Further Exploration

Try these experiments:
1. Same query, different hardware profiles
2. Adding/removing indexes (simulated)
3. Varying data sizes
4. Different join conditions
5. Nested vs unnested subqueries

## Conclusion

The interactive plan visualization tool provides a powerful way to:
- Understand query execution
- Compare optimization strategies
- Learn database internals
- Identify performance bottlenecks
- Experiment with different approaches

Start with simple queries and progressively explore more complex scenarios. Use the comparison mode to understand optimization tradeoffs, and leverage different hardware profiles to see how execution strategies adapt to available resources.

For questions or issues, refer to the main documentation or check the browser console for debugging information.

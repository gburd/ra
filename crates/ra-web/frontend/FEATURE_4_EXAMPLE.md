# Feature 4: Raw Plan View - Visual Examples

## Example: Basic EXPLAIN Output

### Input (from API)
```
QUERY PLAN
Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32)
  Filter: (department_id = 1)

Engine: postgresql-16
Query: SELECT * FROM employees WHERE department_id = 1
```

### Display (Syntax Highlighted)

```
QUERY PLAN
Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32) [Cost: 35.50]
  Filter: (department_id = 1)

Engine: postgresql-16
Query: SELECT * FROM employees WHERE department_id = 1
```

**Visual Appearance:**
- "Seq Scan" appears in cyan (#4ec9b0) and bold
- "cost", "rows", "width" appear in green (#b5cea8)
- Numbers (0.00, 35.50, 2550, 32) appear in green (#b5cea8)
- A green badge shows "Cost: 35.50" next to the operation
- "Filter" appears in cyan (#4ec9b0) and bold
- Indentation preserved (2 spaces per level)

## Example: EXPLAIN ANALYZE Output

### Input (from API)
```
QUERY PLAN (EXPLAIN ANALYZE)
Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32) (actual time=0.012..0.234 rows=1000 loops=1)
  Filter: (department_id = 1)
  Rows Removed by Filter: 500
Planning Time: 0.123 ms
Execution Time: 0.456 ms

Engine: postgresql-16
Query: SELECT * FROM employees WHERE department_id = 1
```

### Display (Syntax Highlighted with Badges)

```
QUERY PLAN (EXPLAIN ANALYZE)
[>] Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32) (actual time=0.012..0.234 rows=1000 loops=1) [Cost: 35.50] [Time: 0.23ms]
      Filter: (department_id = 1)
      Rows Removed by Filter: 500
Planning Time: 0.123 ms [Planning: 0.12ms]
Execution Time: 0.456 ms [Execution: 0.46ms]

Engine: postgresql-16
Query: SELECT * FROM employees WHERE department_id = 1
```

**Visual Appearance:**
- Collapsible arrow icon [>] before operation (can toggle to expand/collapse children)
- "Seq Scan" in cyan (#4ec9b0) and bold
- Metrics in green (#b5cea8)
- Green badge "Cost: 35.50"
- Orange badge "Time: 0.23ms"
- Blue badges for "Planning: 0.12ms" and "Execution: 0.46ms"
- Child operations indented

## Example: Search Highlighting

### Search term: "Filter"

```
QUERY PLAN
Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32)
  [Filter]: (department_id = 1)      ← Current match (orange #ff9632)
  Rows Removed by [Filter]: 500      ← Other match (gold #ffd700)
```

**Search Bar Display:**
```
┌─────────────────────────────────────────────────────────────┐
│ [Filter_______________] "1 of 2"  [↑] [↓] [×]              │
└─────────────────────────────────────────────────────────────┘
```

## Example: Complex Nested Plan

### Input (Collapsed)
```
[>] Hash Join  (cost=45.50..100.25 rows=500 width=64) [Cost: 100.25] [Time: 1.23ms]
[>] Hash  (cost=35.50..35.50 rows=800 width=32) [Cost: 35.50] [Time: 0.45ms]
[>] Seq Scan on orders  (cost=0.00..50.00 rows=2000 width=32) [Cost: 50.00] [Time: 0.78ms]
```

### Input (Expanded)
```
[v] Hash Join  (cost=45.50..100.25 rows=500 width=64) [Cost: 100.25] [Time: 1.23ms]
      Hash Cond: (orders.customer_id = customers.id)
    [v] Hash  (cost=35.50..35.50 rows=800 width=32) [Cost: 35.50] [Time: 0.45ms]
        [>] Seq Scan on customers  (cost=0.00..35.50 rows=800 width=32) [Cost: 35.50]
              Filter: (active = true)
    [v] Seq Scan on orders  (cost=0.00..50.00 rows=2000 width=32) [Cost: 50.00] [Time: 0.78ms]
          Filter: (order_date > '2024-01-01')
```

**Controls:**
```
┌─────────────────────────────────────────────────────────────┐
│ [Expand All] [Collapse All]                                  │
└─────────────────────────────────────────────────────────────┘
```

## Example: Time Formatting

The `formatTime` utility formats timing values for readability:

| Raw Value (ms) | Formatted Display |
|---------------|-------------------|
| 0.001         | 1µs              |
| 0.123         | 123µs            |
| 1.234         | 1.23ms           |
| 12.345        | 12.35ms          |
| 123.456       | 123.46ms         |
| 1234.567      | 1.23s            |
| 13000.0       | 13.00s           |

## Example: Number Formatting

The `formatNumber` utility adds thousands separators:

| Raw Value | Formatted Display |
|-----------|-------------------|
| 123       | 123              |
| 1234      | 1,234            |
| 1234567   | 1,234,567        |
| 1234567890| 1,234,567,890    |

## UI Components Layout

```
┌─────────────────────────────────────────────────────────────┐
│ OutputPanel                                                  │
├─────────────────────────────────────────────────────────────┤
│ [Engine: PostgreSQL 16 v] [🔍] [📋]                         │ ← Toolbar
├─────────────────────────────────────────────────────────────┤
│ [Search________] "3 of 15"  [↑] [↓] [×]                     │ ← SearchBar (toggled)
├─────────────────────────────────────────────────────────────┤
│ [Expand All] [Collapse All]                                  │ ← PlanViewer controls
├─────────────────────────────────────────────────────────────┤
│                                                              │
│   QUERY PLAN (EXPLAIN ANALYZE)                              │
│   [v] Hash Join  (cost=45.50..100.25) [Cost: 100.25]       │
│         Hash Cond: (orders.customer_id = customers.id)      │
│       [v] Hash  (cost=35.50..35.50) [Cost: 35.50]          │
│           [>] Seq Scan on customers                         │
│       [v] Seq Scan on orders  [Cost: 50.00] [Time: 0.78ms] │
│             Filter: (order_date > '2024-01-01')            │
│   Planning Time: 0.123 ms [Planning: 0.12ms]               │
│   Execution Time: 1.456 ms [Execution: 1.46ms]             │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## User Interactions

### 1. Toggle Search
- Click search icon (🔍) in toolbar
- SearchBar slides down
- Focus automatically placed in search input

### 2. Search Through Plan
- Type "Filter" in search input
- All matches highlighted in gold
- First match highlighted in orange
- Counter shows "1 of 2"
- Press Enter or click [↓] to go to next match
- Press Shift+Enter or click [↑] to go to previous match
- Press Escape or click [×] to close search

### 3. Navigate Tree Structure
- Click [>] icon to expand collapsed operation
- Click [v] icon to collapse expanded operation
- Click "Expand All" to show entire tree
- Click "Collapse All" to collapse all operations

### 4. Copy Plan
- Click copy icon (📋) in toolbar
- Toast appears at bottom: "Copied to clipboard"
- Toast disappears after 2 seconds
- Plain text (no highlighting) copied to clipboard

### 5. Scroll to Match
- When navigating search matches, view automatically scrolls
- Current match centered in viewport
- Smooth scroll animation

## Accessibility

- All interactive elements keyboard accessible
- Icon buttons have tooltips
- ARIA labels on custom controls
- Semantic HTML structure
- High contrast color scheme (WCAG AA compliant)

## Performance

- Large plans (>10,000 lines) render efficiently with virtualization
- Search implemented with regex (fast even on large plans)
- Collapse state stored in React state (instant toggling)
- Syntax highlighting applied during render (no re-parsing)
- Match references stored in refs (efficient scrolling)

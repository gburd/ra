# Example: Simple Optimization

This example demonstrates a basic query optimization using predicate pushdown.

## Original Query

```sql
SELECT c.name, c.email
FROM customers c
JOIN orders o ON c.id = o.customer_id
WHERE o.amount > 1000;
```

## Initial Query Plan

```
Project [c.name, c.email]
  `--- Filter [o.amount > 1000]
      `--- Join [c.id = o.customer_id]
          |--- Scan [customers] -> 10,000 rows
          `--- Scan [orders] -> 100,000 rows
```

**Cost Analysis:**
- Scan customers: 10,000 rows
- Scan orders: 100,000 rows
- Join: 10,000 $\times$ 100,000 = 1,000,000,000 tuple comparisons
- Filter: 1,000,000 result rows (assuming 1% join selectivity)
- Project: 1,000,000 rows

**Estimated Cost:** ~1,001,010,000 operations

## Optimization Steps

### Step 1: Filter Pushdown

Rule applied: `filter-through-join` (right side)

The filter `o.amount > 1000` only references the `orders` table, so we can push it down before the join:

```
Project [c.name, c.email]
  `--- Join [c.id = o.customer_id]
      |--- Scan [customers] -> 10,000 rows
      `--- Filter [o.amount > 1000]
          `--- Scan [orders] -> 100,000 rows
```

Now the filter is applied immediately after scanning orders.

### Step 2: Further Pushdown

Rule applied: `filter-into-scan`

We can push the filter into the scan itself (using an index or storage-level filtering):

```
Project [c.name, c.email]
  `--- Join [c.id = o.customer_id]
      |--- Scan [customers] -> 10,000 rows
      `--- Scan [orders WHERE amount > 1000] -> 5,000 rows
```

### Step 3: Column Pruning

Rule applied: `column-pruning`

We only need `customer_id` and `amount` from orders, not all columns:

```
Project [c.name, c.email]
  `--- Join [c.id = o.customer_id]
      |--- Project [c.id, c.name, c.email]
      |   `--- Scan [customers] -> 10,000 rows
      `--- Project [o.customer_id]
          `--- Scan [orders WHERE amount > 1000] -> 5,000 rows
```

## Final Optimized Plan

```
Project [c.name, c.email]
  `--- Join [c.id = o.customer_id]
      |--- Project [c.id, c.name, c.email]
      |   `--- Scan [customers] -> 10,000 rows
      `--- Project [o.customer_id]
          `--- Scan [orders WHERE amount > 1000] -> 5,000 rows
```

**Cost Analysis:**
- Scan orders (filtered): 5,000 rows (90% filtered out)
- Scan customers: 10,000 rows
- Join: 10,000 $\times$ 5,000 = 50,000,000 tuple comparisons
- Project: ~5,000 result rows (assuming 50% customers have high-value orders)

**Estimated Cost:** ~50,015,000 operations

**Cost Reduction:** 95% (from 1,001,010,000 to 50,015,000)

## Rules Applied

1. **filter-through-join** - Pushed filter through join
2. **filter-into-scan** - Merged filter into scan
3. **column-pruning** - Removed unnecessary columns

## Code Example

```rust
use ra_core::{RelExpr, Expr};
use ra_parser::load_rules;
use ra_engine::optimize;

fn main() -> anyhow::Result<()> {
    // Build the original query plan
    let query = RelExpr::Project {
        cols: vec![
            Expr::column("c.name"),
            Expr::column("c.email"),
        ],
        input: Box::new(RelExpr::Filter {
            pred: Expr::gt(
                Expr::column("o.amount"),
                Expr::const_i64(1000)
            ),
            input: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: Expr::eq(
                    Expr::column("c.id"),
                    Expr::column("o.customer_id")
                ),
                left: Box::new(RelExpr::Scan {
                    table: "customers".to_string(),
                }),
                right: Box::new(RelExpr::Scan {
                    table: "orders".to_string(),
                }),
            }),
        }),
    };

    // Load optimization rules
    let rules = load_rules("rules/")?;

    // Optimize
    let optimized = optimize(query, &rules)?;

    println!("Original cost: {}", query.estimate_cost(&stats));
    println!("Optimized cost: {}", optimized.estimate_cost(&stats));
    println!("Improvement: {}%",
        (1.0 - optimized.estimate_cost(&stats) / query.estimate_cost(&stats)) * 100.0
    );

    Ok(())
}
```

## Running the Example

```bash
# Using the CLI
ra-cli optimize "
SELECT c.name, c.email
FROM customers c
JOIN orders o ON c.id = o.customer_id
WHERE o.amount > 1000
"

# With explanation
ra-cli explain "
SELECT c.name, c.email
FROM customers c
JOIN orders o ON c.id = o.customer_id
WHERE o.amount > 1000
"
```

## Output

```
Original Plan:
  Project
    Filter
      Join
        Scan(customers)
        Scan(orders)

Optimized Plan:
  Project
    Join
      Project
        Scan(customers)
      Project
        Scan(orders WHERE amount > 1000)

Rules Applied:
  1. filter-through-join (benefit: 85%)
  2. filter-into-scan (benefit: 5%)
  3. column-pruning (benefit: 5%)

Cost Reduction: 95%
Estimated Speedup: 20x
```

## Key Takeaways

1. **Predicate pushdown** is one of the most effective optimizations
2. **Filter early, filter often** - Reduce data volume as soon as possible
3. **Column pruning** reduces I/O and memory usage
4. **Multiple rules compose** - Small improvements add up

## Next Steps

- [Complex Join Ordering Example](complex-join-order.md)
- [Subquery Unnesting Example](subquery-unnesting.md)
- [Rule Authoring Guide](../rule-authoring.md)

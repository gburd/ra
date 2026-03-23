# Rule: Grouping Sets Expansion

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/grouping-sets-expansion.rra`

## Metadata

- **ID:** `grouping-sets-expansion`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, clickhouse, cockroachdb, mssql, oracle
- **Tags:** aggregation, cube, rollup
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(grouping-sets ?input ?sets ?aggs)"
    description: "GROUPING SETS / CUBE / ROLLUP aggregation"
  - type: "predicate"
    condition: "count(?sets) > 1"
    description: "Multiple grouping sets for expansion benefit"
  - type: "capability"
    database: "current"
    requires: "grouping_sets"
    description: "Database supports GROUPING SETS syntax"
```


# Grouping Sets Expansion

## Metadata
- **Rule ID**: `grouping-sets-expansion`
- **Category**: Physical / Aggregation Strategies
- **Complexity**: O(n * 2^k) where k = number of grouping dimensions
- **Introduced**: SQL:1999 (CUBE, ROLLUP, GROUPING SETS)
- **Prerequisites**: Support for multiple grouping levels
- **Alternatives**: UNION ALL of separate GROUP BYs

## Description

Grouping sets compute multiple GROUP BY aggregations in a single pass. CUBE generates all 2^n combinations, ROLLUP generates hierarchical subtotals, GROUPING SETS specifies exact combinations.

**Syntax:**
- `GROUPING SETS ((a,b), (a), ())` - Specific combinations
- `ROLLUP (a, b, c)` - Hierarchical: (a,b,c), (a,b), (a), ()
- `CUBE (a, b)` - All combinations: (a,b), (a), (b), ()

**When to use:**
- OLAP reporting with subtotals
- Multi-dimensional analysis
- Avoiding multiple scans for different groupings

## Relational Algebra

```
GROUPING SETS ((g1, g2), (g1), ())
-> $\gamma$_{g1,g2; AGG}(R) $\cup$ $\gamma$_{g1; AGG}(R) $\cup$ $\gamma$_{; AGG}(R)

Optimized:
-> GroupingSetsAgg(R, [(g1,g2), (g1), ()], AGG)
  // Single scan, multiple hash tables
```

## Implementation

```rust
pub struct GroupingSetsAggregation {
    input: Box<dyn Operator>,
    grouping_sets: Vec<Vec<usize>>, // Each set is column indexes
    agg_funcs: Vec<AggregateFunction>,
    hash_tables: Vec<HashMap<GroupKey, AggState>>,
    current_set: usize,
    emitting: bool,
}

impl Operator for GroupingSetsAggregation {
    fn next(&mut self) -> Option<Tuple> {
        if \!self.emitting {
            // Build phase: single scan, update all hash tables
            while let Some(tuple) = self.input.next() {
                for (i, grouping_set) in self.grouping_sets.iter().enumerate() {
                    let key = self.extract_group_key(&tuple, grouping_set);
                    self.hash_tables[i]
                        .entry(key)
                        .or_insert_with(|| self.init_state())
                        .update(&tuple);
                }
            }
            self.emitting = true;
        }

        // Emit phase: emit from each hash table in sequence
        while self.current_set < self.hash_tables.len() {
            if let Some((key, state)) = self.hash_tables[self.current_set].iter().next() {
                let tuple = self.finalize_group(key, state, self.current_set);
                return Some(tuple);
            }
            self.current_set += 1;
        }

        None
    }
}
```

## Cost Model

```rust
pub fn cost_grouping_sets(
    input_card: u64,
    grouping_sets: &[Vec<Column>],
    hardware: &HardwareModel,
) -> Cost {
    let num_sets = grouping_sets.len();

    // Single scan cost
    let scan_cost = Cost::io(
        (input_card as f64 / hardware.tuples_per_page()) * hardware.sequential_page_read_cost()
    );

    // Update cost: update N hash tables per tuple
    let update_cost = Cost::cpu(input_card * num_sets as u64 * 10);

    // Memory cost: N hash tables
    let memory_cost = Cost::memory(
        grouping_sets.iter()
            .map(|gs| estimate_group_cardinality(input_card, gs) * 64)
            .sum()
    );

    scan_cost + update_cost + memory_cost
}
```

## Test Cases

### Test 1: ROLLUP for hierarchical subtotals
```sql
SELECT year, quarter, month, SUM(revenue)
FROM sales
GROUP BY ROLLUP (year, quarter, month);

-- Generates:
-- (year, quarter, month) - Detail
-- (year, quarter)        - Quarterly subtotals
-- (year)                 - Yearly subtotals
-- ()                     - Grand total
```

### Test 2: CUBE for multi-dimensional analysis
```sql
SELECT region, product, SUM(sales)
FROM facts
GROUP BY CUBE (region, product);

-- Generates all 2^2 = 4 combinations:
-- (region, product) - By region and product
-- (region)          - By region only
-- (product)         - By product only
-- ()                - Grand total
```

### Test 3: GROUPING SETS for specific combinations
```sql
SELECT city, category, SUM(amount)
FROM transactions
GROUP BY GROUPING SETS ((city, category), (city), ());

-- Generates exactly 3 groupings:
-- (city, category), (city), ()
-- More efficient than CUBE which would generate (category) too
```

### Test 4: GROUPING function to distinguish NULLs
```sql
SELECT
  city,
  category,
  SUM(amount),
  GROUPING(city) as city_is_total,
  GROUPING(category) as cat_is_total
FROM transactions
GROUP BY ROLLUP (city, category);

-- GROUPING() returns 1 for aggregated NULL, 0 for real NULL
```

## References

1. **SQL:1999 Standard**: CUBE, ROLLUP, GROUPING SETS specification
2. **Microsoft mssql**: GROUPING SETS implementation
   - https://docs.microsoft.com/sql/t-sql/queries/select-group-by-transact-sql
3. **Oracle**: GROUP BY Extensions
   - https://docs.oracle.com/database/grouping-sets.html
4. **PostgreSQL**: GROUPING SETS
   - https://www.postgresql.org/docs/current/queries-table-expressions.html#QUERIES-GROUPING-SETS

## Tags
`physical`, `aggregation`, `grouping-sets`, `olap`, `cube`, `rollup`

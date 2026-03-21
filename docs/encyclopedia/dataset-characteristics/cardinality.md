# Cardinality

## Description

Cardinality measures the number of distinct values in a column. One of the most critical statistics for query optimization, affecting index selection, join algorithms, and cardinality estimation.

## Categories

### High Cardinality
- **Definition:** Distinct values ≈ row count
- **Examples:** Primary keys, UUIDs, email addresses, URLs
- **Ratio:** $\frac{\text{distinct}}{\text{total}} > 0.9$

### Medium Cardinality
- **Definition:** Thousands to millions of distinct values
- **Examples:** User IDs, product IDs, zip codes
- **Ratio:** $0.01 < \frac{\text{distinct}}{\text{total}} \leq 0.9$

### Low Cardinality
- **Definition:** Tens to hundreds of distinct values
- **Examples:** Status flags, countries, categories, gender
- **Ratio:** $\frac{\text{distinct}}{\text{total}} \leq 0.01$

## Impact on Optimization

### Index Selection

$$
\text{Index Utility} = \frac{\text{Selectivity} \times \text{Distinct Count}}{\text{Total Rows}}
$$

| Cardinality | B-tree Index | Hash Index | Bitmap Index |
|-------------|--------------|-----------|--------------|
| High | ✅ Excellent | ✅ Excellent | ❌ Too large |
| Medium | ✅ Good | ✅ Good | ⚠️ Situational |
| Low | ⚠️ Situational | ❌ Poor | ✅ Excellent |

**Rule:** `physical/index-type-selection`

Ra chooses index type based on:

$$
\text{Bitmap if: } \text{distinct} < 1000 \text{ AND } \text{query has multiple predicates}
$$

$$
\text{Hash if: } \text{queries are equality only}
$$

$$
\text{B-tree otherwise (default)}
$$

### Join Cardinality Estimation

For equi-join $R \bowtie_{R.a = S.b} S$:

$$
|R \bowtie S| = \frac{|R| \times |S|}{\max(\text{distinct}(R.a), \text{distinct}(S.b))}
$$

**Assumption:** Uniform distribution within each relation.

**With correlation awareness:**

$$
|R \bowtie S| = |R| \times |S| \times \text{sel}(R.a = S.b) \times (1 + \text{corr}(R.a, S.b))
$$

### Aggregation Method Selection

**Rule:** `physical/aggregation-method-selection`

$$
\text{Hash Agg if: } \text{distinct}(G) \times \text{row\_size} < \text{work\_mem}
$$

$$
\text{Sort Agg otherwise}
$$

Where $G$ are grouping columns.

## Statistics API

```rust
use ra_optimizer::{Statistics, ColumnStatistics};

// High cardinality column (primary key)
optimizer.add_column_stats("users", "id", ColumnStatistics {
    distinct_count: 10_000_000,  // Equal to row count
    null_fraction: 0.0,
    min_value: Some(1),
    max_value: Some(10_000_000),
});

// Medium cardinality column (foreign key)
optimizer.add_column_stats("orders", "customer_id", ColumnStatistics {
    distinct_count: 500_000,  // 500K distinct customers
    null_fraction: 0.0,
});

// Low cardinality column (status)
optimizer.add_column_stats("orders", "status", ColumnStatistics {
    distinct_count: 5,  // [pending, processing, shipped, delivered, cancelled]
    null_fraction: 0.0,
    most_common_values: vec![
        ("delivered", 0.50),
        ("shipped", 0.25),
        ("pending", 0.15),
        ("processing", 0.08),
        ("cancelled", 0.02),
    ],
});

// Very low cardinality (boolean)
optimizer.add_column_stats("users", "is_active", ColumnStatistics {
    distinct_count: 2,
    null_fraction: 0.0,
    most_common_values: vec![
        ("true", 0.85),
        ("false", 0.15),
    ],
});
```

## Examples

### High Cardinality: Point Lookup

```sql
SELECT * FROM users WHERE email = 'user@example.com';
```

**Ra Plan:**

```
IndexScan [users.email_btree_idx]
  Filter: email = 'user@example.com'
```

**Selectivity:** $\frac{1}{\text{distinct}(\text{email})} = \frac{1}{10{,}000{,}000} = 0.0000001$

**Cost:** $O(\log n)$ with B-tree index.

### Low Cardinality: Bitmap Index

```sql
SELECT * FROM orders
WHERE status IN ('pending', 'processing')
  AND priority = 'high';
```

**Ra Plan:**

```
BitmapHeapScan [orders]
  BitmapAnd
    BitmapIndexScan [orders.status_bitmap_idx]
      (status IN ('pending', 'processing'))
    BitmapIndexScan [orders.priority_bitmap_idx]
      (priority = 'high')
```

**Advantage:** Bitmap indexes compress well for low cardinality, fast AND/OR operations.

**Selectivity:**

$$
\text{sel} = (0.15 + 0.08) \times 0.10 = 0.023 \quad (2.3\% \text{ of rows})
$$

### Medium Cardinality: Join

```sql
SELECT c.name, COUNT(*) as order_count
FROM customers c
JOIN orders o ON o.customer_id = c.id
GROUP BY c.name;
```

**Cardinality Estimation:**

$$
|orders \bowtie customers| = \frac{10{,}000{,}000 \times 500{,}000}{\max(500{,}000, 500{,}000)} = 10{,}000{,}000
$$

Join cardinality equals orders table size (many-to-one join).

**Ra Plan:**

```
HashAggregate [name]
  Aggregates: COUNT(*)
  HashJoin [o.customer_id = c.id]
    SeqScan [customers c]  -- Build side (500K rows)
    SeqScan [orders o]     -- Probe side (10M rows)
```

### Cardinality-Aware Filter Ordering

```sql
SELECT *
FROM users
WHERE country = 'USA'          -- Low cardinality, 30% selectivity
  AND subscription_type = 'premium'  -- Low cardinality, 5% selectivity
  AND created_at > '2024-01-01'      -- Medium cardinality, 20% selectivity
  AND email LIKE '%@gmail.com';      -- High cardinality, 15% selectivity
```

**Rule:** `logical/predicate-reorder`

Ra evaluates predicates by selectivity:

$$
\text{Order: } \text{subscription\_type} \rightarrow \text{email} \rightarrow \text{created\_at} \rightarrow \text{country}
$$

**Selectivity Calculation:**

$$
\text{sel}_{\text{combined}} = 0.05 \times 0.15 \times 0.20 \times 0.30 = 0.00045 \quad (0.045\%)
$$

**Ra Plan:**

```
SeqScan [users]
  Filter:
    subscription_type = 'premium'  -- Most selective (5%)
    AND email LIKE '%@gmail.com'   -- Next (15%)
    AND created_at > '2024-01-01'  -- Then (20%)
    AND country = 'USA'            -- Least selective (30%)
```

## Cardinality Estimation Formulas

### Selection Predicate

$$
\text{sel}(\text{col} = v) = \begin{cases}
\frac{1}{\text{distinct}(\text{col})} & \text{if uniform distribution} \\
\text{MCV frequency}(v) & \text{if most common value} \\
\frac{1 - \sum \text{MCV frequencies}}{\text{distinct} - |\text{MCV}|} & \text{otherwise}
\end{cases}
$$

### Range Predicate

$$
\text{sel}(\text{low} \leq \text{col} \leq \text{high}) = \frac{\text{high} - \text{low}}{\text{max} - \text{min}}
$$

For non-uniform distribution, use histogram:

$$
\text{sel} = \sum_{i: b_i \in [\text{low}, \text{high}]} f_i
$$

### LIKE Predicate

$$
\text{sel}(\text{col LIKE } '\%\text{pattern}\%') \approx \frac{1}{\sqrt{\text{distinct}(\text{col})}}
$$

Leading wildcard prevents index usage.

## Cardinality Drift

Cardinality changes over time:

```rust
// Stale statistics (10M rows, collected 1 month ago)
optimizer.add_column_stats("users", "id", ColumnStatistics {
    distinct_count: 10_000_000,
    collection_timestamp: "2024-02-01",
});

// Actual current size: 12M rows
// Estimation error: 20%
```

**Solution:** Ra supports adaptive statistics:

```rust
use ra_optimizer::AdaptiveStatistics;

// Actual cardinality observed during execution
optimizer.update_runtime_stats("users", RuntimeStats {
    actual_row_count: 12_000_000,
    execution_timestamp: "2024-03-01",
});

// Ra adjusts future estimates
```

## Special Cases

### NULL Handling

```sql
SELECT * FROM users WHERE country IS NULL;
```

**Selectivity:**

$$
\text{sel}(\text{col IS NULL}) = \text{null\_fraction}
$$

**Distinct Count Adjustment:**

$$
\text{distinct}(\text{col}) = \text{distinct\_non\_null} + (1 \text{ if nulls exist})
$$

### Functional Dependencies

If columns are functionally dependent:

$$
\text{city} \rightarrow \text{country}
$$

Then:

$$
\text{distinct}(\text{city, country}) = \text{distinct}(\text{city})
$$

Not:

$$
\text{distinct}(\text{city}) \times \text{distinct}(\text{country})
$$

Ra's correlation-aware estimator detects this.

## Performance Impact

| Operation | High Cardinality | Low Cardinality |
|-----------|-----------------|----------------|
| Point lookup | Fast (index efficient) | Slow (low selectivity) |
| Range scan | Moderate | Fast (few distinct values) |
| Hash aggregation | Large hash table | Small hash table |
| Sort aggregation | Expensive sort | Cheap sort |
| Bitmap index | Huge bitmap | Compact bitmap |
| Join estimation | Accurate | May overestimate |

## See Also

- [Distribution](distribution.md) - Value distribution patterns
- [Skew](skew.md) - Imbalanced data
- [Correlation](correlation.md) - Column dependencies
- [Index Structures: Bitmap](../index-structures/bitmap.md) - Low-cardinality indexes
- [Index Structures: B-tree](../index-structures/btree.md) - High-cardinality indexes
- [Rule: Cardinality Estimation](../../rules/cost-models/cardinality-estimation.md)

## References

- Selinger et al., "Access Path Selection in a Relational Database", *SIGMOD 1979*
- Ioannidis, "The History of Histograms", *VLDB 2003*
- Leis et al., "How Good Are Query Optimizers, Really?", *VLDB 2015*

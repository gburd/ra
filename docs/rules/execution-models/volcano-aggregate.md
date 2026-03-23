# Rule: Volcano Iterator Model - Hash Aggregation

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-aggregate.rra`

## Metadata

- **ID:** `volcano-aggregate`
- **Version:** 1.0.0
- **Databases:** PostgreSQL, MySQL
- **Tags:** execution, iterator, volcano, aggregate, group-by
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Hash Aggregation

## Description

Hash-based GROUP BY aggregation. Materializes input into hash table keyed by grouping columns, then iterates over final aggregates. Pipeline breaker.

## Relational Algebra

```
Aggregate(input, group_keys, agg_funcs) -> Iterator<Tuple>

AggregateIterator {
  input: Iterator
  agg_table: HashMap<Key, AggState>
  result_iter: Iterator

  fn open() {
    // Build aggregation
    while tuple = input.next() {
      key = extract_group_key(tuple)
      agg_table[key].update(tuple)
    }
    result_iter = agg_table.values().iter()
  }

  fn next() -> Tuple | None {
    result_iter.next()
  }
}
```

## Implementation

```rust
pub struct AggregateIterator {
    input: Box<dyn Iterator<Item = Tuple>>,
    group_keys: Vec<Expr>,
    aggregates: Vec<AggregateExpr>,
    agg_table: HashMap<GroupKey, AggregateState>,
    results: Vec<Tuple>,
    position: usize,
}

impl Iterator for AggregateIterator {
    fn open(&mut self) -> Result<()> {
        self.input.open()?;

        // Build aggregation table
        while let Some(tuple) = self.input.next()? {
            let key = extract_group_key(&tuple, &self.group_keys)?;
            let state = self.agg_table.entry(key).or_default();
            update_aggregates(state, &tuple, &self.aggregates)?;
        }
        self.input.close()?;

        // Finalize aggregates
        for (key, state) in &self.agg_table {
            self.results.push(finalize_aggregate(key, state)?);
        }
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        if self.position < self.results.len() {
            let result = self.results[self.position].clone();
            self.position += 1;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
}

pub fn aggregate_cost(input_rows: f64, num_groups: f64) -> f64 {
    input_rows * 0.001 + num_groups * 0.0005
}
```

## Cost Model

- **CPU:** O(N) hash updates + O(G) finalizations
- **Memory:** O(G) for hash table
- **Pipeline breaker:** Must see all input

## Test Cases

```sql
SELECT region, COUNT(*), SUM(amount) FROM orders GROUP BY region;
SELECT user_id, MAX(score) FROM games GROUP BY user_id;
```

## References

1. Graefe, "Volcano", IEEE TKDE 1994
2. Larson, "Data Reduction by Partial Preaggregation", ICDE 2002

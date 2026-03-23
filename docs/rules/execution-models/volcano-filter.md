# Rule: Volcano Iterator Model - Filter

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-filter.rra`

## Metadata

- **ID:** `volcano-filter`
- **Version:** 1.0.0
- **Databases:** PostgreSQL, MySQL, Oracle, SQLite
- **Tags:** execution, iterator, volcano, filter, selection
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Filter

## Description

Filter operator in Volcano model evaluates predicates tuple-by-tuple. Only tuples satisfying the predicate are passed to parent operators, enabling pipelined execution without materialization.

## Relational Algebra

```
Filter(input, predicate) -> Iterator<Tuple>

FilterIterator implements Iterator {
  input: Iterator
  predicate: Expr

  fn next() -> Tuple | None {
    loop {
      tuple = input.next()
      if tuple == None { return None }
      if eval(predicate, tuple) {
        return tuple
      }
    }
  }
}
```

## Implementation

```rust
pub struct FilterIterator {
    input: Box<dyn Iterator<Item = Tuple>>,
    predicate: Expr,
}

impl Iterator for FilterIterator {
    type Item = Tuple;

    fn next(&mut self) -> Result<Option<Tuple>> {
        loop {
            match self.input.next()? {
                None => return Ok(None),
                Some(tuple) => {
                    if eval_predicate(&self.predicate, &tuple)? {
                        return Ok(Some(tuple));
                    }
                }
            }
        }
    }
}

pub fn filter_cost(input_rows: f64, selectivity: f64) -> f64 {
    input_rows * 0.0001 // Cost per predicate evaluation
}
```

## Cost Model

- **CPU:** `input_rows $\times$ predicate_complexity`
- **I/O:** Zero (pipelined from child)
- **Memory:** O(1)
- **Output:** `input_rows $\times$ selectivity`

## Test Cases

```sql
-- High selectivity filter
SELECT * FROM orders WHERE status = 'pending';

-- Complex predicate
SELECT * FROM users WHERE age > 18 AND region IN ('US', 'CA') AND active = true;

-- Always false predicate
SELECT * FROM items WHERE 1 = 0;
```

## References

1. Graefe, "Volcano: An Extensible and Parallel Query Evaluation System", IEEE TKDE 1994
2. PostgreSQL: `src/backend/executor/nodeFilter.c`

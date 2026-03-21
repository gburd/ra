# Rule: Volcano Iterator Model - External Sort

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-sort.rra`

## Metadata

- **ID:** `volcano-sort`
- **Version:** 1.0.0
- **Databases:** PostgreSQL, MySQL, Oracle
- **Tags:** execution, iterator, volcano, sort, external-sort
- **Authors:** Goetz Graefe


# Volcano Iterator Model - External Sort

## Description

External merge sort for ORDER BY. Materializes input, sorts (possibly spilling to disk), then returns tuples in sorted order. Pipeline breaker.

## Relational Algebra

```
Sort(input, keys) → Iterator<Tuple>

SortIterator {
  input: Iterator
  sorted_tuples: Vec<Tuple>
  position: usize

  fn open() {
    // Materialize and sort
    tuples = collect_all(input)
    sorted_tuples = external_sort(tuples, keys)
    position = 0
  }

  fn next() → Tuple | None {
    if position < sorted_tuples.len() {
      return sorted_tuples[position++]
    }
    None
  }
}
```

## Implementation

```rust
pub struct SortIterator {
    input: Box<dyn Iterator<Item = Tuple>>,
    sort_keys: Vec<SortKey>,
    sorted_tuples: Vec<Tuple>,
    position: usize,
}

impl Iterator for SortIterator {
    fn open(&mut self) -> Result<()> {
        // Collect all tuples
        let mut tuples = Vec::new();
        self.input.open()?;
        while let Some(t) = self.input.next()? {
            tuples.push(t);
        }
        self.input.close()?;

        // Sort
        tuples.sort_by(|a, b| compare_tuples(a, b, &self.sort_keys));
        self.sorted_tuples = tuples;
        self.position = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        if self.position < self.sorted_tuples.len() {
            let tuple = self.sorted_tuples[self.position].clone();
            self.position += 1;
            Ok(Some(tuple))
        } else {
            Ok(None)
        }
    }
}

pub fn sort_cost(rows: f64, row_size: usize) -> f64 {
    rows * (rows.log2()) * 0.001 // O(N log N) comparisons
}
```

## Cost Model

- **CPU:** O(N log N) comparisons
- **Memory:** O(N) if fits in memory
- **I/O:** O(N log N / B) if spilling to disk
- **Pipeline breaker:** Must materialize input

## Test Cases

```sql
SELECT * FROM orders ORDER BY order_date DESC, amount;
SELECT name FROM users ORDER BY created_at LIMIT 10;
```

## References

1. Graefe, "Implementing Sorting in Database Systems", ACM Computing Surveys 2006
2. Knuth, "The Art of Computer Programming Vol 3: Sorting"

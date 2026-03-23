# Rule: Volcano Iterator Model - Hash Join

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-hash-join.rra`

## Metadata

- **ID:** `volcano-hash-join`
- **Version:** 1.0.0
- **Databases:** PostgreSQL, MySQL, Oracle
- **Tags:** execution, iterator, volcano, join, hash-join
- **SQL Standard:** Volcano model
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Hash Join

## Description

Hash join builds a hash table from the inner relation during `open()`, then probes it for each outer tuple in `next()`. Efficient for equijoins with O(N+M) complexity.

## Relational Algebra

```
HashJoin(R, S, R.a = S.b) -> Iterator<Tuple>

HashJoinIterator {
  build_input: Iterator  // Inner (smaller)
  probe_input: Iterator  // Outer
  hash_table: HashMap<Key, Vec<Tuple>>
  current_matches: Vec<Tuple>

  fn open() {
    // Build phase
    build_input.open()
    while tuple = build_input.next() {
      key = extract_key(tuple)
      hash_table[key].push(tuple)
    }
    build_input.close()
    probe_input.open()
  }

  fn next() -> Tuple | None {
    // Return buffered matches first
    if !current_matches.empty() {
      return current_matches.pop()
    }

    // Probe next outer tuple
    outer = probe_input.next()
    if outer == None { return None }

    key = extract_key(outer)
    if hash_table.contains(key) {
      current_matches = hash_table[key]
      return merge(outer, current_matches.pop())
    }

    // No match, continue
    return next()
  }
}
```

## Implementation

```rust
pub struct HashJoinIterator {
    build_input: Box<dyn Iterator<Item = Tuple>>,
    probe_input: Box<dyn Iterator<Item = Tuple>>,
    hash_table: HashMap<JoinKey, Vec<Tuple>>,
    current_outer: Option<Tuple>,
    current_matches: Vec<Tuple>,
}

impl Iterator for HashJoinIterator {
    fn open(&mut self) -> Result<()> {
        // Build phase
        self.build_input.open()?;
        while let Some(tuple) = self.build_input.next()? {
            let key = extract_join_key(&tuple)?;
            self.hash_table.entry(key).or_default().push(tuple);
        }
        self.build_input.close()?;

        self.probe_input.open()?;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<Tuple>> {
        loop {
            // Return buffered matches
            if let Some(inner) = self.current_matches.pop() {
                let outer = self.current_outer.as_ref().unwrap();
                return Ok(Some(merge_tuples(outer, &inner)));
            }

            // Get next outer tuple
            self.current_outer = self.probe_input.next()?;
            if self.current_outer.is_none() {
                return Ok(None);
            }

            // Probe hash table
            let key = extract_join_key(self.current_outer.as_ref().unwrap())?;
            if let Some(matches) = self.hash_table.get(&key) {
                self.current_matches = matches.clone();
            }
        }
    }
}

pub fn hash_join_cost(
    build_rows: f64,
    probe_rows: f64,
    selectivity: f64,
) -> f64 {
    let build_cost = build_rows * 0.001; // Hash table construction
    let probe_cost = probe_rows * 0.0005; // Hash lookup
    let output_cost = (probe_rows * selectivity) * 0.0005; // Merge
    build_cost + probe_cost + output_cost
}
```

## Cost Model

- **Build Phase:** O(M) to construct hash table
- **Probe Phase:** O(N) with O(1) lookups
- **Memory:** O(M) for hash table
- **Total:** O(N + M) time, O(M) space

## Test Cases

```sql
-- Equijoin
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;

-- Multiple join keys
SELECT * FROM sales s JOIN products p ON s.product_id = p.id AND s.store_id = p.store_id;

-- Large build side (suboptimal)
SELECT * FROM small_table s JOIN large_table l ON s.id = l.small_id;
```

## References

1. Graefe, "Volcano", IEEE TKDE 1994
2. Shapiro, "Join Processing in Database Systems with Large Main Memories", ACM TODS 1986

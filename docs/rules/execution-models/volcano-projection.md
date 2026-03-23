# Rule: Volcano Iterator Model - Projection

**Category:** execution-models
**File:** `rules/execution-models/volcano/volcano-projection.rra`

## Metadata

- **ID:** `volcano-projection`
- **Version:** 1.0.0
- **Databases:** PostgreSQL, MySQL, Oracle
- **Tags:** execution, iterator, volcano, projection
- **Authors:** Goetz Graefe


# Volcano Iterator Model - Projection

## Description

Project operator selects and computes output columns. Evaluates expressions per tuple and constructs result tuples. Fully pipelined.

## Relational Algebra

```
Project(input, expressions) -> Iterator<Tuple>

ProjectIterator {
  input: Iterator
  exprs: Vec<Expr>

  fn next() -> Tuple | None {
    tuple = input.next()
    if tuple == None { return None }

    result = Tuple::new()
    for expr in exprs {
      result.add(eval(expr, tuple))
    }
    return result
  }
}
```

## Implementation

```rust
pub struct ProjectIterator {
    input: Box<dyn Iterator<Item = Tuple>>,
    projections: Vec<ProjectionColumn>,
}

impl Iterator for ProjectIterator {
    fn next(&mut self) -> Result<Option<Tuple>> {
        match self.input.next()? {
            None => Ok(None),
            Some(input_tuple) => {
                let mut result = Tuple::new();
                for proj in &self.projections {
                    let value = eval_expr(&proj.expr, &input_tuple)?;
                    result.add_column(proj.alias.as_deref(), value);
                }
                Ok(Some(result))
            }
        }
    }
}

pub fn projection_cost(rows: f64, num_exprs: usize) -> f64 {
    rows * num_exprs as f64 * 0.0001
}
```

## Cost Model

- **CPU:** O(N $\times$ E) where E = number of expressions
- **Memory:** O(1)
- **Pipelined:** Yes

## Test Cases

```sql
SELECT name, email FROM users;
SELECT price * quantity AS total FROM items;
SELECT UPPER(name), age + 1 FROM people;
```

## References

1. Graefe, "Volcano", IEEE TKDE 1994

# Rule: "Starburst Contradiction Detection and Unsatisfiable Query Elimination"

**Category:** logical/expression-simplification
**File:** `rules/logical/expression-simplification/starburst-contradiction-detection.rra`

## Metadata

- **ID:** `starburst-contradiction-detection`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, cockroachdb, mssql, oracle
- **Tags:** contradiction, unsatisfiable, empty-result, constraint-detection, starburst, classic
- **Authors:** "Pirahesh, Hellerstein, Hasan (Starburst); King (QUIST)"


# Starburst Contradiction Detection and Unsatisfiable Query Elimination

## Description

Detects queries (or query fragments) whose predicates are logically
contradictory, meaning no row can possibly satisfy them. When a contradiction
is detected, the entire subtree can be replaced with an empty result, avoiding
all computation for that branch.

Starburst's semantic optimization component checks predicates against each
other and against schema constraints (CHECK, NOT NULL, domain constraints)
to detect contradictions. The QUIST system (King, 1981) pioneered this
approach, and Starburst generalized it as part of its rule-based rewrite
framework.

Types of contradictions detected:
1. **Direct**: WHERE x = 1 AND x = 2
2. **Range**: WHERE x > 10 AND x < 5
3. **Constraint-based**: WHERE price < 0 (when CHECK(price >= 0))
4. **Type-based**: WHERE date_col = 'not-a-date' (domain violation)
5. **NULL**: WHERE x IS NULL AND x > 0
6. **Partition**: WHERE partition_key = 'A' on partition CHECK(key = 'B')

**When to apply**: During query rewrite, before cost-based optimization.
Early detection avoids planning and executing guaranteed-empty queries.

**Why it works**: If no row can satisfy the combined predicates, the query
produces no output regardless of the data. Detecting this statically avoids
scanning any tables, building any hash tables, or performing any joins.

## Relational Algebra

```algebra
Contradiction detection rules:

1. Equality contradiction:
   sigma_{a = v1 AND a = v2}(R) => empty  (when v1 != v2)

2. Range contradiction:
   sigma_{a > v1 AND a < v2}(R) => empty  (when v1 >= v2)
   sigma_{a >= v1 AND a <= v2}(R) => empty  (when v1 > v2)

3. NULL contradiction:
   sigma_{a IS NULL AND a = v}(R) => empty  (for any value v)
   sigma_{a IS NULL AND a > v}(R) => empty  (for any comparison)

4. CHECK constraint contradiction:
   sigma_{a < 0}(R) => empty  (when CHECK(a >= 0) on R)

5. IN contradiction:
   sigma_{a IN (1,2,3) AND a IN (4,5,6)}(R) => empty  (empty intersection)

6. NOT NULL contradiction:
   sigma_{a IS NULL}(R) => empty  (when a is NOT NULL)

7. Propagated contradiction through equijoin:
   sigma_{A.x = B.y AND A.x = 1 AND B.y = 2}(A join B) => empty
   (equijoin + conflicting constants)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("contradiction-eq-eq";
    "(filter (and (= ?col ?v1) (= ?col ?v2)) ?input)" =>
    "(empty)"
    if values_differ("?v1", "?v2")
),

rw!("contradiction-range";
    "(filter (and (> ?col ?v1) (< ?col ?v2)) ?input)" =>
    "(empty)"
    if value_gte("?v1", "?v2")
),

rw!("contradiction-null-comparison";
    "(filter (and (is-null ?col) (> ?col ?v)) ?input)" =>
    "(empty)"
),

rw!("contradiction-not-null-is-null";
    "(filter (is-null ?col) ?input)" =>
    "(empty)"
    if column_not_null("?col")
),

rw!("contradiction-check-constraint";
    "(filter ?pred ?input)" =>
    "(empty)"
    if contradicts_check_constraint("?pred", "?input")
),

rw!("contradiction-in-lists";
    "(filter (and (in ?col ?list1) (in ?col ?list2)) ?input)" =>
    "(empty)"
    if in_lists_disjoint("?list1", "?list2")
),

// Full contradiction detector

struct ContradictionDetector {
    constraints: ConstraintCatalog,
}

impl ContradictionDetector {
    fn is_contradictory(
        &self,
        predicates: &[Predicate],
        table: &Table,
    ) -> bool {
        // Collect all constraints on each column
        let mut column_constraints: HashMap<Column, Vec<Constraint>> =
            HashMap::new();

        for pred in predicates {
            for col in pred.referenced_columns() {
                column_constraints
                    .entry(col.clone())
                    .or_default()
                    .push(Constraint::from_predicate(pred, &col));
            }
        }

        // Add schema constraints
        for col in column_constraints.keys() {
            if let Some(checks) =
                self.constraints.check_constraints(col)
            {
                column_constraints
                    .get_mut(col)
                    .unwrap()
                    .extend(checks.iter().cloned());
            }
            if self.constraints.is_not_null(col) {
                column_constraints
                    .get_mut(col)
                    .unwrap()
                    .push(Constraint::NotNull);
            }
        }

        // Check each column for contradictions
        for (col, constraints) in &column_constraints {
            if self.constraints_contradictory(constraints) {
                return true;
            }
        }

        // Check cross-column contradictions (equijoin transitivity)
        self.check_equijoin_contradictions(predicates)
    }

    fn constraints_contradictory(
        &self,
        constraints: &[Constraint],
    ) -> bool {
        // Build the feasible value range
        let mut range = ValueRange::full();

        for c in constraints {
            match c {
                Constraint::Eq(v) => {
                    if !range.contains(v) {
                        return true; // Value outside feasible range
                    }
                    range = ValueRange::point(v.clone());
                }
                Constraint::Gt(v) => {
                    range = range.intersect_gt(v);
                }
                Constraint::Lt(v) => {
                    range = range.intersect_lt(v);
                }
                Constraint::Gte(v) => {
                    range = range.intersect_gte(v);
                }
                Constraint::Lte(v) => {
                    range = range.intersect_lte(v);
                }
                Constraint::IsNull => {
                    if range.excludes_null() {
                        return true;
                    }
                    range = ValueRange::null_only();
                }
                Constraint::NotNull => {
                    if range.is_null_only() {
                        return true;
                    }
                    range = range.exclude_null();
                }
                Constraint::In(values) => {
                    let filtered: Vec<_> = values.iter()
                        .filter(|v| range.contains(v))
                        .collect();
                    if filtered.is_empty() {
                        return true; // No IN values in range
                    }
                }
                _ => {}
            }

            if range.is_empty() {
                return true;
            }
        }

        false
    }

    fn check_equijoin_contradictions(
        &self,
        predicates: &[Predicate],
    ) -> bool {
        // Build equivalence classes from equalities
        let mut eq_classes: UnionFind<Column> = UnionFind::new();
        let mut constants: HashMap<Column, Value> = HashMap::new();

        for pred in predicates {
            match pred {
                Predicate::Eq(col, Value::Const(v)) => {
                    constants.insert(col.clone(), v.clone());
                }
                Predicate::Eq(
                    Value::Col(c1),
                    Value::Col(c2),
                ) => {
                    eq_classes.union(c1, c2);
                }
                _ => {}
            }
        }

        // Check if any equivalence class has conflicting constants
        for (col, val) in &constants {
            let class = eq_classes.find(col);
            for member in eq_classes.members(&class) {
                if let Some(other_val) = constants.get(member) {
                    if val != other_val {
                        return true; // Conflicting constants
                    }
                }
            }
        }

        false
    }
}
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    hw: &HardwareProfile,
) -> bool {
    // Always applicable -- contradiction detection is cheap
    // and the benefit (avoiding execution) is huge
    stats.has_multiple_predicates
        || stats.has_check_constraints
}
```

**Restrictions:**
- Can only detect statically provable contradictions
- Requires type information for range comparisons
- CHECK constraints must be enforced and trusted
- Complex predicates (e.g., involving functions) may not be analyzable
- Correlated predicates require per-row evaluation (cannot detect statically)

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    _hw: &HardwareProfile,
) -> f64 {
    // If contradiction detected: infinite benefit (avoid entire execution)
    // Detection cost is O(p^2) where p is number of predicates
    if stats.contradiction_detected {
        1.0 // 100% benefit: skip all execution
    } else {
        0.0 // No contradiction: no benefit
    }
}
```

**Detection cost**: O(p^2) where p is the number of predicates.
**Benefit when detected**: 100% -- skip all I/O, CPU, and network costs.

## Test Cases

### Positive: Direct equality contradiction

```sql
SELECT * FROM orders
WHERE status = 'shipped' AND status = 'pending';

-- 'shipped' != 'pending' -> contradiction!
-- Result: Empty (no scan needed)
```

### Positive: Range contradiction

```sql
SELECT * FROM products
WHERE price > 1000 AND price < 500;

-- 1000 >= 500 -> empty range -> contradiction!
-- Result: Empty
```

### Positive: CHECK constraint contradiction

```sql
-- CHECK(quantity >= 0) on order_items
SELECT * FROM order_items
WHERE quantity < 0;

-- Contradicts CHECK constraint
-- Result: Empty
```

### Positive: Transitive contradiction through equijoin

```sql
SELECT * FROM orders o
JOIN status_codes s ON o.status_id = s.id
WHERE o.status_id = 1 AND s.id = 2;

-- o.status_id = s.id AND o.status_id = 1 AND s.id = 2
-- Transitively: 1 = 2 -> contradiction!
-- Result: Empty
```

### Positive: IN list with no overlap

```sql
SELECT * FROM products
WHERE category IN ('A', 'B', 'C')
  AND category IN ('X', 'Y', 'Z');

-- Intersection of {A,B,C} and {X,Y,Z} is empty
-- Result: Empty
```

### Positive: NOT NULL with IS NULL

```sql
-- id is PRIMARY KEY (NOT NULL)
SELECT * FROM customers WHERE id IS NULL;

-- NOT NULL constraint contradicts IS NULL
-- Result: Empty
```

### Negative: Non-contradictory predicates

```sql
SELECT * FROM orders
WHERE status = 'shipped' AND total > 100;

-- No contradiction: orders can be both shipped and > $100
```

### Positive: Partition pruning produces empty

```sql
-- Partition: orders_2023 has CHECK(year = 2023)
-- Partition: orders_2024 has CHECK(year = 2024)
SELECT * FROM orders_2023 WHERE year = 2024;

-- CHECK(year = 2023) contradicts year = 2024
-- This specific partition returns empty
-- Critical for partition pruning in UNION ALL views
```

## References

**Original papers:**
- King, J.J., "QUIST: A System for Semantic Query Optimization in Relational Databases", VLDB 1981
  - Pioneered contradiction detection using constraints

- Pirahesh, H., Hellerstein, J.M., Hasan, W., "Extensible/Rule Based Query Rewrite Optimization in Starburst", ACM SIGMOD 1992
  - DOI: 10.1145/130283.130294
  - Section 4.4: Unsatisfiable query detection

- Chakravarthy, U.S., Grant, J., Minker, J., "Logic-Based Approach to Semantic Query Optimization", ACM TODS 1990
  - DOI: 10.1145/78922.78924
  - Formal framework for contradiction detection

**Implementation in databases:**
- PostgreSQL: `src/backend/optimizer/util/predtest.c` - predicate_refuted_by()
  - Also: constraint exclusion for partitions
- MySQL: Constant propagation and impossible WHERE detection
- Oracle: Constraint-based optimization pass
- mssql: Contradiction detection in Cascades optimizer

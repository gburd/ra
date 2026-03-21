# Rule: Column-at-a-Time Projection and Expression Evaluation

**Category:** execution-models
**File:** `rules/execution-models/column-at-a-time/column-projection.rra`

## Metadata

- **ID:** `column-projection`
- **Version:** 1.0.0
- **Databases:** MonetDB, ClickHouse, DuckDB, vectorwise, vertica
- **Tags:** execution, columnar, x100, projection, expression, zero-copy, simd
- **SQL Standard:** MonetDB X100
- **Authors:** Peter Boncz, Marcin Zukowski


# Column-at-a-Time Projection and Expression Evaluation

## Description

Column-at-a-time projection selects and computes output columns from input column arrays. Simple column references are zero-copy (pass pointer to existing column array). Computed expressions (arithmetic, string operations, CASE) produce new column arrays by evaluating the expression over the entire input column in a tight loop. This amortizes function call overhead across thousands of values and enables SIMD vectorization of arithmetic operations.

**Projection types:**
1. **Column reference**: Zero-copy pointer to existing column -- O(0) cost
2. **Constant**: Broadcast scalar to column array -- O(N) but trivial
3. **Arithmetic expression**: Evaluate element-wise on columns -- O(N) with SIMD
4. **Type cast**: Convert column data type -- O(N) per value
5. **String expression**: Evaluate on variable-length data -- O(N x avg_len)
6. **CASE/conditional**: Branch per value or branchless -- O(N)

**Key characteristics:**
- **Zero-copy column references**: No data movement for simple SELECT columns
- **Tight evaluation loops**: One function call evaluates entire column
- **SIMD arithmetic**: 4-8 operations per cycle for numeric types
- **Temporary column reuse**: Intermediate columns allocated from a pool
- **No per-tuple overhead**: Function dispatch happens once per column, not per tuple

**MonetDB BAT algebra:**
- `BATcalc.+(A, B)` produces new BAT with element-wise addition
- `BATconvert(A, type)` produces new BAT with type conversion
- Operations are bulk: input and output are full column arrays
- Interpreter dispatches one function per column operation

**Trade-offs:**
- Intermediate columns consume memory (column_length x type_width each)
- Complex expressions with many intermediates increase memory pressure
- Variable-length types (strings) break SIMD patterns
- Expression with mixed types requires materialization at each cast boundary

## Relational Algebra

```
ColumnProject([expr1, expr2, ...], input_columns)
  = [eval(expr1, input_columns), eval(expr2, input_columns), ...]

-- Zero-copy reference:
  eval(ColRef(i), columns) = columns[i]  -- pointer copy

-- Arithmetic:
  eval(Add(e1, e2), columns) =
    column_add(eval(e1, columns), eval(e2, columns))
```

## Implementation

```rust
/// Column-at-a-time projection operator
pub struct ColumnProjection {
    /// Output expressions (one per output column)
    expressions: Vec<ProjectExpr>,
}

pub enum ProjectExpr {
    /// Direct column reference -- zero copy
    ColumnRef(ColumnId),
    /// Constant value broadcast to column
    Constant(ScalarValue),
    /// Arithmetic: op(left, right)
    BinaryOp {
        op: ArithOp,
        left: Box<ProjectExpr>,
        right: Box<ProjectExpr>,
    },
    /// Unary operation (negate, abs, cast)
    UnaryOp {
        op: UnaryOp,
        input: Box<ProjectExpr>,
    },
    /// Conditional: CASE WHEN cond THEN a ELSE b
    Conditional {
        condition: Box<ProjectExpr>,
        then_expr: Box<ProjectExpr>,
        else_expr: Box<ProjectExpr>,
    },
}

impl ColumnProjection {
    /// Evaluate all projection expressions on input columns
    pub fn evaluate(
        &self,
        input: &[ColumnArray],
        sel: Option<&SelectionVector>,
    ) -> Vec<ColumnArray> {
        self.expressions.iter().map(|expr| {
            eval_expr(expr, input, sel)
        }).collect()
    }
}

/// Recursive expression evaluator (column-at-a-time)
fn eval_expr(
    expr: &ProjectExpr,
    input: &[ColumnArray],
    sel: Option<&SelectionVector>,
) -> ColumnArray {
    match expr {
        ProjectExpr::ColumnRef(col_id) => {
            // Zero-copy: return reference to input column
            // If selection vector, gather only needed positions
            match sel {
                Some(sv) => gather(&input[*col_id], sv),
                None => input[*col_id].clone_ref(),
            }
        }

        ProjectExpr::Constant(val) => {
            let len = sel.map(|s| s.positions.len())
                .unwrap_or(input[0].len);
            broadcast_scalar(val, len)
        }

        ProjectExpr::BinaryOp { op, left, right } => {
            let left_col = eval_expr(left, input, sel);
            let right_col = eval_expr(right, input, sel);
            column_binary_op(&left_col, &right_col, *op)
        }

        ProjectExpr::UnaryOp { op, input: child } => {
            let child_col = eval_expr(child, input, sel);
            column_unary_op(&child_col, *op)
        }

        ProjectExpr::Conditional {
            condition, then_expr, else_expr,
        } => {
            let cond_col = eval_expr(condition, input, sel);
            let then_col = eval_expr(then_expr, input, sel);
            let else_col = eval_expr(else_expr, input, sel);
            column_conditional(
                &cond_col, &then_col, &else_col,
            )
        }
    }
}

/// SIMD column addition (i64 + i64 -> i64)
fn column_add_i64(
    left: &ColumnArray,
    right: &ColumnArray,
) -> ColumnArray {
    let left_data = left.data.as_i64_slice();
    let right_data = right.data.as_i64_slice();
    let len = left.len;

    let mut result = AlignedBuffer::new(len * 8);
    let result_data = result.as_i64_slice_mut();

    // SIMD path: 4 additions per AVX2 instruction
    #[cfg(target_arch = "x86_64")]
    {
        let chunks = len / 4;
        for i in 0..chunks {
            let offset = i * 4;
            unsafe {
                let a = _mm256_loadu_si256(
                    left_data[offset..].as_ptr() as *const _,
                );
                let b = _mm256_loadu_si256(
                    right_data[offset..].as_ptr() as *const _,
                );
                let sum = _mm256_add_epi64(a, b);
                _mm256_storeu_si256(
                    result_data[offset..].as_mut_ptr() as *mut _,
                    sum,
                );
            }
        }
        // Scalar remainder
        for i in (chunks * 4)..len {
            result_data[i] = left_data[i] + right_data[i];
        }
    }

    // Null handling: null if either input is null
    let null_bitmap = merge_null_bitmaps(
        &left.null_bitmap, &right.null_bitmap,
    );

    ColumnArray {
        col_id: 0, // temporary
        data_type: DataType::Int64,
        data: result,
        null_bitmap,
        len,
    }
}

/// Branchless CASE WHEN: select from two columns
fn column_conditional(
    cond: &ColumnArray,
    then_col: &ColumnArray,
    else_col: &ColumnArray,
) -> ColumnArray {
    let cond_data = cond.data.as_bool_slice();
    let then_data = then_col.data.as_i64_slice();
    let else_data = else_col.data.as_i64_slice();

    let mut result = AlignedBuffer::new(cond.len * 8);
    let result_data = result.as_i64_slice_mut();

    for i in 0..cond.len {
        // Branchless: mask-based selection
        let mask = -(cond_data[i] as i64); // 0 or -1
        result_data[i] =
            (then_data[i] & mask) | (else_data[i] & !mask);
    }

    ColumnArray {
        col_id: 0,
        data_type: then_col.data_type,
        data: result,
        null_bitmap: None,
        len: cond.len,
    }
}
```

## Cost Model

**Per-Column Expression Cost:**
- Column reference (zero-copy): 0 ns
- Constant broadcast: ~0.25 ns/value (memory store)
- SIMD arithmetic (i32/i64): ~0.25 ns/value (4 ops/cycle AVX2)
- Scalar arithmetic: ~1 ns/value
- Type cast (int->float): ~1 ns/value
- String concatenation: ~50-200 ns/value
- CASE (branchless): ~1 ns/value
- CASE (branchy): ~1-5 ns/value (branch misprediction)

**Memory:**
- Each intermediate column: `N x type_width` bytes
- Expression `(a + b) * (c - d)`: 3 intermediate columns
- Pool allocation amortizes malloc overhead

**Throughput (1M i64 values, single core):**
- `a + b`: ~0.25 ms (SIMD) / ~1 ms (scalar)
- `a + b * c - d / 2`: ~1 ms (SIMD, 4 intermediates)
- `CASE WHEN a > 0 THEN b ELSE c`: ~1.5 ms

## Test Cases

```sql
-- Test 1: Zero-copy column reference
SELECT name, dept FROM employees;
-- Zero data movement: pointers to existing column arrays
-- Cost: essentially free

-- Test 2: Arithmetic expression
SELECT id, price * quantity AS total FROM line_items;
-- Evaluate price * quantity on full columns
-- SIMD: 4 multiplications per cycle (i64)
-- One intermediate column for result

-- Test 3: Complex expression with intermediates
SELECT id, (price * quantity * (1 - discount)) * (1 + tax_rate)
FROM line_items;
-- Intermediate columns: t1=price*quantity, t2=1-discount,
--   t3=t1*t2, t4=1+tax_rate, result=t3*t4
-- 5 column operations, 5 intermediate allocations

-- Test 4: Mixed types with casts
SELECT id, CAST(amount AS DOUBLE) / CAST(count AS DOUBLE)
FROM metrics;
-- Cast int columns to double, then divide
-- 2 cast operations + 1 division = 3 column ops
```

## Comparison

| Property | Column-at-a-Time | Tuple-at-a-Time | Compiled |
|----------|-----------------|-----------------|----------|
| Function dispatch | 1 per column | 1 per tuple | 0 (inlined) |
| SIMD utilization | Natural | None | Possible |
| Intermediate storage | Full columns | 1 value | Registers |
| Zero-copy refs | Yes | No | N/A |
| Memory pressure | High (columns) | Low (1 tuple) | Low (registers) |
| Interpretation overhead | Low (amortized) | High (per tuple) | None |

## References

1. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
2. **Zukowski, Marcin**. "Balancing Vectorized Query Execution with Bandwidth-Optimized Storage." PhD Thesis, CWI, 2009.
3. **Kersten, Timo; Leis, Viktor; Kemper, Alfons; Neumann, Thomas; Pavlo, Andrew; Boncz, Peter**. "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask." VLDB 2018.
4. **Sompolski, Jan; Zukowski, Marcin; Boncz, Peter**. "Vectorization vs. Compilation in Query Execution." DaMoN 2011.

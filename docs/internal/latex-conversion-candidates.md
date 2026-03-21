# LaTeX Conversion Candidates

This document identifies mathematical expressions in rule files that would benefit from LaTeX formatting for improved clarity and readability.

## Executive Summary

Analyzed 1354+ rule files across cost models, physical operators, logical rewrites, and execution models. Found **hundreds of mathematical formulas** that would benefit from LaTeX conversion, particularly in:

1. **Cost model formulas** (highest priority)
2. **Cardinality estimation equations**
3. **Complexity notation**
4. **Selectivity formulas**
5. **Join cost calculations**
6. **Physical operator complexity bounds**

## High-Priority Conversions

### 1. Cost Models (Critical)

#### File: `rules/cost-models/system-r-cost-formula.rra`
**Lines: 18-19, 44-71**

Current plain text:
```
COST = PAGE_FETCHES + W * RSI_CALLS

Sequential Scan(R):
  PAGE_FETCHES = N_pages(R)
  RSI_CALLS    = N_tuples(R)

Index Scan(R, I, selectivity F):
  If I is clustered:
    PAGE_FETCHES = F * N_pages(R)
  Else:
    PAGE_FETCHES = min(F * N_tuples(R), N_pages(R))
  RSI_CALLS    = F * N_tuples(R)

Sort-Merge Join(R, S):
  PAGE_FETCHES = sort_pages(R) + sort_pages(S) + PAGES(R) + PAGES(S)
  RSI_CALLS    = TUPLES(R) + TUPLES(S)

Sort(R):
  PAGE_FETCHES = 2 * N_pages(R) * ceil(log_B(N_pages(R)))
  RSI_CALLS    = N_tuples(R) * ceil(log2(N_tuples(R)))
```

Should be LaTeX:
```latex
$$\text{COST} = \text{PAGE\_FETCHES} + W \times \text{RSI\_CALLS}$$

Sequential Scan:
$$\text{PAGE\_FETCHES} = N_{\text{pages}}(R)$$
$$\text{RSI\_CALLS} = N_{\text{tuples}}(R)$$

Index Scan with selectivity $F$:
- Clustered: $\text{PAGE\_FETCHES} = F \cdot N_{\text{pages}}(R)$
- Unclustered: $\text{PAGE\_FETCHES} = \min(F \cdot N_{\text{tuples}}(R), N_{\text{pages}}(R))$
$$\text{RSI\_CALLS} = F \cdot N_{\text{tuples}}(R)$$

Sort-Merge Join:
$$\text{PAGE\_FETCHES} = \text{sort\_pages}(R) + \text{sort\_pages}(S) + \text{PAGES}(R) + \text{PAGES}(S)$$
$$\text{RSI\_CALLS} = \text{TUPLES}(R) + \text{TUPLES}(S)$$

Sort:
$$\text{PAGE\_FETCHES} = 2 \cdot N_{\text{pages}}(R) \cdot \lceil \log_B(N_{\text{pages}}(R)) \rceil$$
$$\text{RSI\_CALLS} = N_{\text{tuples}}(R) \cdot \lceil \log_2(N_{\text{tuples}}(R)) \rceil$$
```

**Impact**: Foundation formula used throughout database systems. Critical for understanding cost-based optimization.

---

#### File: `rules/cost-models/system-r-selectivity-formulas.rra`
**Lines: 40-56**

Current plain text:
```
1. col = value           -> F = 1 / ICARD(R.A)
2. col1 = col2           -> F = 1 / max(ICARD(R.A), ICARD(R.B))
3. col > value           -> F = (high_key - value) / (high_key - low_key)
4. col BETWEEN v1 AND v2 -> F = (v2 - v1) / (high_key - low_key)
5. col IN (v1, ..., vn)  -> F = min(n / ICARD(R.A), 0.5)
6. P1 AND P2             -> F = F(P1) * F(P2)
7. P1 OR P2              -> F = F(P1) + F(P2) - F(P1) * F(P2)
8. NOT P                 -> F = 1 - F(P)
```

Should be LaTeX:
```latex
System R Selectivity Formulas:

1. Equality: $\text{col} = \text{value} \Rightarrow F = \frac{1}{\text{ICARD}(R.A)}$

2. Join equality: $\text{col}_1 = \text{col}_2 \Rightarrow F = \frac{1}{\max(\text{ICARD}(R.A), \text{ICARD}(R.B))}$

3. Range: $\text{col} > \text{value} \Rightarrow F = \frac{\text{high\_key} - \text{value}}{\text{high\_key} - \text{low\_key}}$

4. BETWEEN: $\text{col} \text{ BETWEEN } v_1 \text{ AND } v_2 \Rightarrow F = \frac{v_2 - v_1}{\text{high\_key} - \text{low\_key}}$

5. IN list: $\text{col} \in \{v_1, \ldots, v_n\} \Rightarrow F = \min\left(\frac{n}{\text{ICARD}(R.A)}, 0.5\right)$

6. Conjunction: $P_1 \land P_2 \Rightarrow F = F(P_1) \times F(P_2)$

7. Disjunction: $P_1 \lor P_2 \Rightarrow F = F(P_1) + F(P_2) - F(P_1) \times F(P_2)$

8. Negation: $\neg P \Rightarrow F = 1 - F(P)$
```

**Impact**: Core selectivity estimation formulas used in every database optimizer.

---

#### File: `rules/cost-models/join-cost-formulas.rra`
**Lines: 45-76**

Current plain text:
```
1. Nested-Loop Join (NLJ):
   C_io(NLJ) = P_R + |R| * P_S                     (no index)
   C_io(NLJ) = P_R + |R| * (h + F*P_S/|S|)         (with index)
   C_cpu(NLJ) = |R| * |S| * c_compare               (no index)
   C_cpu(NLJ) = |R| * (c_hash + F*|S|/D * c_compare) (with index)

2. Sort-Merge Join (SMJ):
   C_io(SMJ) = sort_io(R) + sort_io(S) + P_R + P_S
   sort_io(X) = 0 if X sorted, else 2*P_X*ceil(log_M(P_X/M))
   C_cpu(SMJ) = sort_cpu(R) + sort_cpu(S) + (|R| + |S|) * c_compare

3. Hash Join (HJ):
   C_io(HJ) = P_R + P_S                             (in-memory)
   C_io(HJ) = 3*(P_R + P_S)                         (one-pass Grace)
   C_io(HJ) = 2*(P_R + P_S)*ceil(log_M(P_R/M)) + P_R + P_S  (recursive)
   C_cpu(HJ) = (|R| + |S|) * c_hash + |S| * c_probe
```

Should be LaTeX:
```latex
**1. Nested-Loop Join (NLJ):**

No index:
$$C_{\text{io}}(\text{NLJ}) = P_R + |R| \times P_S$$
$$C_{\text{cpu}}(\text{NLJ}) = |R| \times |S| \times c_{\text{compare}}$$

With index (height $h$, selectivity $F$):
$$C_{\text{io}}(\text{NLJ}) = P_R + |R| \times \left(h + \frac{F \cdot P_S}{|S|}\right)$$
$$C_{\text{cpu}}(\text{NLJ}) = |R| \times \left(c_{\text{hash}} + \frac{F \cdot |S|}{D} \times c_{\text{compare}}\right)$$

**2. Sort-Merge Join (SMJ):**
$$C_{\text{io}}(\text{SMJ}) = \text{sort\_io}(R) + \text{sort\_io}(S) + P_R + P_S$$

Where:
$$\text{sort\_io}(X) = \begin{cases}
0 & \text{if } X \text{ sorted} \\
2 \cdot P_X \cdot \lceil \log_M(P_X/M) \rceil & \text{otherwise}
\end{cases}$$

$$C_{\text{cpu}}(\text{SMJ}) = \text{sort\_cpu}(R) + \text{sort\_cpu}(S) + (|R| + |S|) \times c_{\text{compare}}$$

**3. Hash Join (HJ):**

In-memory:
$$C_{\text{io}}(\text{HJ}) = P_R + P_S$$

Grace (one-pass):
$$C_{\text{io}}(\text{HJ}) = 3 \times (P_R + P_S)$$

Recursive partitioning:
$$C_{\text{io}}(\text{HJ}) = 2 \times (P_R + P_S) \times \lceil \log_M(P_R/M) \rceil + P_R + P_S$$

CPU cost:
$$C_{\text{cpu}}(\text{HJ}) = (|R| + |S|) \times c_{\text{hash}} + |S| \times c_{\text{probe}}$$
```

**Impact**: Fundamental join cost formulas used for join algorithm selection.

---

### 2. Physical Operator Complexity

#### Files with O(n) notation needing LaTeX:

1. **`rules/physical/parallelization/work-stealing-parallelism.rra`** (Lines 11, 20, 45)
   - Current: `O(n/p + log p)`
   - LaTeX: `$O(n/p + \log p)$`

2. **`rules/physical/parallelization/degree-of-parallelism-selection.rra`** (Line 237)
   - Current: `dop_opt = sqrt(cost / overhead)`
   - LaTeX: `$\text{DOP}_{\text{opt}} = \sqrt{\frac{\text{cost}}{\text{overhead}}}$`

3. **`rules/physical/sort/sort-spill-partitioning.rra`** (Line 9, 49, 67)
   - Current: `O(n * log_B(n/M))`
   - LaTeX: `$O(n \cdot \log_B(n/M))$`

4. **`rules/physical/aggregation/aggregate-pushdown-through-join.rra`** (Lines 84-85)
   - Current: `join_before = fact_rows * dim_rows.log2()`
   - LaTeX: `$C_{\text{join}}^{\text{before}} = n_{\text{fact}} \times \log_2(n_{\text{dim}})$`

5. **`rules/physical/optimizer-framework/cascades-memo-structure.rra`** (Lines 276-277)
   - Current: `Space complexity: O(2^n * p)`, `Time savings: From O(n! * k) to O(2^n * p * k)`
   - LaTeX: Use proper exponentials and factorials

---

### 3. Cardinality Estimation

#### File: `rules/cost-models/cardinality-estimation.rra`
**Status**: Stub file, needs full implementation with formulas

Suggested content with LaTeX:
```latex
## Cardinality Estimation Formulas

**Base cardinality:**
$$|R| = N_{\text{tuples}}(R)$$

**Selection:**
$$|\sigma_p(R)| = |R| \times \text{sel}(p)$$

**Join cardinality (independence assumption):**
$$|R \bowtie_{\theta} S| = |R| \times |S| \times \text{sel}(\theta)$$

**Containment assumption:**
$$|R \bowtie_{R.a = S.b} S| = \frac{|R| \times |S|}{\max(V(R, a), V(S, b))}$$

Where $V(R, a)$ = number of distinct values in column $a$ of relation $R$.

**Aggregation:**
$$|\gamma_{g_1, \ldots, g_k}(R)| \approx \prod_{i=1}^{k} \min(V(R, g_i), |R|)$$

With correlation correction factor when columns are not independent.
```

---

### 4. Set Operations and Mathematical Notation

Multiple files use set notation that should be LaTeX:

- **Union**: `∪` → `$\cup$`
- **Intersection**: `∩` → `$\cap$`
- **Element of**: `∈` → `$\in$`
- **Inequalities**: `≤`, `≥` → `$\leq$`, `$\geq$`
- **Implies**: `->` → `$\Rightarrow$`

---

## Medium-Priority Conversions

### Distributed Query Cost Models

Files in `rules/distributed/` with cost formulas:
- Network transfer cost: `cost = rows * network_latency + data_size / bandwidth`
- Shuffle cost models
- Broadcast vs. partition decisions

### Cache-Aware Models

Files in `rules/physical/hardware/` with cache formulas:
- Cache miss penalties
- NUMA distance calculations
- Prefetch window sizing

### Statistical Formulas

- Histogram-based selectivity (equi-width, equi-depth)
- Correlation coefficients for multi-column statistics
- Zipfian distribution parameters
- Sampling-based estimation confidence intervals

---

## Conversion Guidelines

### When to Use LaTeX

1. **Formulas with division**: Use `\frac{numerator}{denominator}`
2. **Subscripts/superscripts**: Use `_` and `^` (e.g., `$N_{\text{tuples}}$`)
3. **Logarithms**: Use `\log_b(x)` or `\ln(x)`
4. **Set operations**: Use `\cup`, `\cap`, `\in`
5. **Inequalities**: Use `\leq`, `\geq`, `\neq`
6. **Summations/products**: Use `\sum_{i=1}^{n}` and `\prod`
7. **Square roots**: Use `\sqrt{x}`
8. **Ceiling/floor**: Use `\lceil x \rceil`, `\lfloor x \rfloor`
9. **Piecewise functions**: Use `\begin{cases} ... \end{cases}`
10. **Big O notation**: Use `$O(n \log n)$` not `O(n log n)`

### When NOT to Use LaTeX

1. Simple variable names in prose: "the value of x" (not $x$)
2. Code snippets (keep in code blocks)
3. Plain text descriptions without math
4. Single operators in simple expressions: "x + y = z" is fine

### Style Consistency

- Use `\text{}` for multi-letter variable names: `$\text{cost}$` not `$cost$`
- Use `\times` for multiplication, not `*` in display math
- Use `\cdot` for dot product or when `×` is too heavy
- Subscripts should use `\text{}` for words: `$C_{\text{cpu}}$`

---

## Implementation Priority

### Phase 1: Cost Models (Week 1)
- [ ] `system-r-cost-formula.rra` - CRITICAL
- [ ] `system-r-selectivity-formulas.rra` - CRITICAL
- [ ] `join-cost-formulas.rra` - CRITICAL
- [ ] `cpu-cost-model.rra`
- [ ] `io-cost-model.rra`
- [ ] `memory-cost-model.rra`

### Phase 2: Cardinality Estimation (Week 2)
- [ ] `cardinality-estimation.rra` - Complete stub
- [ ] `correlation-aware-estimation.rra`
- [ ] `histogram-based-estimation.rra`
- [ ] `join-cardinality-estimation.rra`

### Phase 3: Physical Operators (Week 3)
- [ ] Parallelism complexity formulas (20+ files)
- [ ] Sort algorithm complexities
- [ ] Join algorithm complexities
- [ ] Aggregation strategies

### Phase 4: Advanced Topics (Week 4)
- [ ] Distributed query costs
- [ ] Hardware-adaptive models
- [ ] Cache-aware formulas
- [ ] NUMA cost models

---

## Testing Strategy

After LaTeX conversion:

1. **Build VitePress docs** - Ensure KaTeX renders correctly
2. **Visual inspection** - Check formulas are readable
3. **Cross-reference** - Verify consistency with cited papers
4. **Link validation** - Ensure no broken references

---

## Tools and References

### LaTeX Math Resources
- KaTeX supported functions: https://katex.org/docs/supported.html
- VitePress math support: https://vitepress.dev/guide/markdown#math-equations
- LaTeX math symbols: http://tug.ctan.org/info/symbols/comprehensive/symbols-a4.pdf

### Database Theory References
- System R papers (Selinger et al., 1979)
- Join algorithm surveys (Graefe, 1993)
- Modern optimizer papers (Leis et al., 2015)

---

## Notes

- **Total files analyzed**: 1354
- **Files with math content**: ~400
- **High-priority conversions**: ~50 files
- **Estimated effort**: 4 weeks for comprehensive conversion
- **Dependencies**: VitePress LaTeX rendering must be working (Task #71)

This report provides a roadmap for systematic LaTeX conversion across the codebase, prioritizing the most impactful formulas first.

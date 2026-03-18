# Task #6: Fill Logical Rule Directories - Progress Tracker

## Overview
Target: 40 logical optimization rules across 4 categories
Status: IN PROGRESS (3/40 completed)

## Completed Rules ✅

### aggregate-pushdown/ (3/10 completed)
1. ✅ **aggregate-through-union.rra** - Push aggregation through UNION ALL
2. ✅ **group-by-pushdown-through-join.rra** - Pre-aggregate before join
3. ⏳ having-to-filter-separation.rra
4. ⏳ distinct-to-group-by.rra
5. ⏳ count-star-optimization.rra
6. ⏳ partial-aggregation-insertion.rra
7. ⏳ min-max-index-scan.rra
8. ⏳ aggregate-with-constant-elimination.rra
9. ⏳ aggregate-over-aggregate-fusion.rra
10. ⏳ aggregate-selectivity-estimation.rra

### join-elimination/ (0/10 completed)
1. ⏳ redundant-join-elimination.rra
2. ⏳ self-join-elimination.rra
3. ⏳ outer-join-to-filter.rra
4. ⏳ join-elimination-via-unique-key.rra
5. ⏳ key-propagation.rra
6. ⏳ left-join-null-rejection.rra
7. ⏳ anti-join-simplification.rra
8. ⏳ degenerate-join-to-filter.rra
9. ⏳ foreign-key-join-elimination.rra
10. ⏳ semi-join-to-inner-join.rra

### limit-pushdown/ (0/7 completed)
1. ⏳ limit-through-union-all.rra
2. ⏳ limit-before-order-by.rra
3. ⏳ limit-with-top-k-join.rra
4. ⏳ limit-through-aggregate.rra
5. ⏳ sort-limit-fusion.rra
6. ⏳ offset-zero-elimination.rra
7. ⏳ limit-through-projection.rra

### subquery-unnesting/ (1/13 completed)
1. ⏳ uncorrelated-subquery-to-join.rra
2. ⏳ scalar-subquery-to-left-join.rra
3. ✅ **in-subquery-to-semi-join.rra** - IN to semi-join transformation
4. ⏳ not-in-to-anti-join.rra
5. ⏳ exists-to-semi-join.rra
6. ⏳ not-exists-to-anti-join.rra
7. ⏳ lateral-join-decorrelation.rra
8. ⏳ apply-to-join.rra
9. ⏳ subquery-hoisting.rra
10. ⏳ correlated-subquery-decorrelation.rra
11. ⏳ subquery-coalescing.rra
12. ⏳ max-1-row-optimization.rra
13. ⏳ subquery-pushdown-through-union.rra

## Rule Template

Each .rra file must include:

### Required Sections
1. **YAML Frontmatter**
   - id, name, category, databases
   - execution_models, hardware, version
   - authors, tags, complexity, benefit_range

2. **Description**
   - What the rule does
   - When to apply
   - Why it works

3. **Relational Algebra**
   - Formal transformation
   - Conditions and constraints

4. **Implementation**
   - Rust code with egg rewrite rules
   - Helper functions and restrictions

5. **Cost Model**
   - Rust function estimating benefit
   - Assumptions and typical benefits

6. **Test Cases**
   - Minimum 3 test cases
   - At least 1 positive, 1 negative
   - Real SQL examples with explanations

7. **References**
   - Academic papers
   - Implementation references

## Quality Standards

- ✅ Each rule file is 200-300 lines
- ✅ Comprehensive documentation
- ✅ 3+ test cases with SQL examples
- ✅ Cost model with concrete benefit estimates
- ✅ References to academic papers and implementations
- ✅ Integration with egg rewrite engine

## Next Steps

### Immediate Priority (High-Impact Rules)
1. exists-to-semi-join.rra - Very common pattern
2. not-exists-to-anti-join.rra - Complements exists
3. redundant-join-elimination.rra - High benefit
4. limit-through-union-all.rra - Common in partitioned queries
5. having-to-filter-separation.rra - Enables filter pushdown

### Medium Priority
- remaining aggregate-pushdown rules
- join-elimination rules
- limit-pushdown rules

### Lower Priority
- Advanced decorrelation rules
- Edge case optimizations

## Estimated Completion

- **Exemplar rules (10-12)**: Create detailed templates
- **Batch generation (28-30)**: Script-generated with consistent structure
- **Review & test**: Validate all 40 rules compile and have correct structure

Total estimated: 37 remaining rules to create

# PostgreSQL Query Planner Analysis
Date: 2026-03-21
Source: PostgreSQL Documentation (Current)
Relevance: HIGH

## Key Optimization Concepts

### 1. Scan Planning Strategies
PostgreSQL implements sophisticated scan selection:

- **Sequential Scan**: Always available baseline
- **Index Scan**: Chosen when:
  - Query restrictions match index keys
  - Operators compatible with index operator class
  - Index provides useful sort ordering

**RA Gap**: Could benefit from more sophisticated index operator class matching

### 2. Join Algorithms
PostgreSQL uses three core join methods:

- **Nested Loop Join**: Efficient with index on inner relation
- **Merge Join**: Requires sorted inputs, single pass through data
- **Hash Join**: Builds hash table on smaller relation

**RA Status**: Has these basic algorithms in `/rules/physical/join-algorithms/`

### 3. Join Order Optimization
Two-tier approach based on relation count:

- **< geqo_threshold (12)**: Near-exhaustive dynamic programming
- **>= geqo_threshold**: Genetic algorithm

**RA Gap**: Missing genetic algorithm for large join graphs (only has dynamic programming)

### 4. Cost-Based Selection
PostgreSQL estimates costs for:
- I/O operations (sequential and random)
- CPU operations
- Memory usage
- Network transfer (distributed)

**RA Status**: Has comprehensive cost models in `/rules/cost-models/`

### 5. Statistics and Selectivity
Uses column statistics for:
- Histogram-based selectivity
- Most common values (MCV) lists
- Null fraction tracking
- Correlation between physical and logical ordering

**RA Status**: Has these in cost models but could expand MCV handling

## Optimization Techniques to Implement

### High Priority
1. **Genetic Query Optimizer for Large Joins**
   - Currently missing for joins > 12 tables
   - Critical for complex analytical queries

2. **Operator Class Aware Index Selection**
   - Match operators to index capabilities
   - Support custom operator classes

3. **Interesting Order Tracking**
   - Track beneficial sort orders through plan
   - Avoid redundant sorts

### Medium Priority
4. **Loose Index Scan**
   - Skip scan for distinct values
   - Efficient for GROUP BY on indexed columns

5. **Parameterized Path Generation**
   - Create paths with runtime parameters
   - Better nested loop join optimization

6. **Bitmap Index Combining**
   - AND/OR multiple bitmap scans
   - More flexible than single index

### Low Priority
7. **Partial Aggregation Pushdown**
   - Push partial aggregates below joins
   - Reduce data movement

8. **Join Removal via Foreign Keys**
   - Eliminate unnecessary joins
   - When foreign key guarantees uniqueness
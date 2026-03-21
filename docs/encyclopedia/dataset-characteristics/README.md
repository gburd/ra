# Dataset Characteristics

Data properties that influence query optimization decisions.

## Characteristics

### [Cardinality](cardinality.md)
Number of distinct values. Critical for index selection and join estimation.

### [Distribution](distribution.md)
How values are distributed: uniform, Zipfian, normal, bimodal.

### [Skew](skew.md)
Imbalanced data distribution causing performance hotspots.

### [Correlation](correlation.md)
Column dependencies affecting cardinality estimation.

### [Null Handling](null-handling.md)
Sparse columns and NULL-aware optimization.

### [String Patterns](string-patterns.md)
String length, prefix similarity, case sensitivity.

### [Numeric Ranges](numeric-ranges.md)
Bounded vs unbounded ranges, precision requirements.

## Impact Matrix

| Characteristic | Index Choice | Join Algorithm | Aggregation Method |
|----------------|-------------|----------------|-------------------|
| High cardinality | B-tree, Hash | Hash join | Hash aggregate |
| Low cardinality | Bitmap | Sort-merge | Sort aggregate |
| Skewed | Salted hash | Broadcast hot keys | Multi-phase |
| Correlated | Multi-column | Reorder joins | Group early |
| Sparse (nulls) | Partial index | Null-safe join | Filter nulls |
| Long strings | Prefix index | Hash join | Hash aggregate |
| Numeric range | B-tree | Range join | Range bucket |

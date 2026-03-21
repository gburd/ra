# Index Structures

Index types, selection criteria, and performance characteristics.

## Index Types

### [B-tree Indexes](btree.md)
Balanced tree for range queries and sorted access. Default index type.

### [Hash Indexes](hash.md)
Hash table for exact-match lookups. Fast for equality predicates.

### [Bitmap Indexes](bitmap.md)
Compressed bitmaps for low-cardinality columns. Excellent for multiple predicates.

### [GiST Indexes](gist.md)
Generalized Search Tree for geometric, full-text, and custom types.

### [GIN Indexes](gin.md)
Generalized Inverted Index for arrays, JSONB, and text search.

### [Covering Indexes](covering.md)
Include all query columns to enable index-only scans.

### [Partial Indexes](partial.md)
Index subset of rows matching predicate. Smaller, faster.

## Selection Matrix

| Cardinality | Predicate Type | Best Index | Reason |
|-------------|---------------|-----------|---------|
| High | Equality | B-tree or Hash | Selective, fast lookup |
| High | Range | B-tree | Supports range scans |
| Low | Equality | Bitmap | Compact, fast AND/OR |
| Low | Range | B-tree | Still works, less optimal |
| Mixed | Multiple ANDs | Bitmap | Efficient combination |
| Any | Covering | Covering B-tree | No heap access |

## Cost Comparison

| Operation | B-tree | Hash | Bitmap | Covering |
|-----------|--------|------|--------|----------|
| Point lookup | $O(\log n)$ | $O(1)$ | $O(1)$ | $O(\log n)$ |
| Range scan | $O(\log n + k)$ | N/A | $O(n)$ | $O(\log n + k)$ |
| Multiple predicates | $O(\log n + k)$ each | N/A | $O(b)$ | $O(\log n + k)$ |
| Index-only scan | $O(\log n + k)$ | N/A | N/A | $O(\log n + k)$ |
| Insert/update | $O(\log n)$ | $O(1)$ | $O(1)$ | $O(\log n)$ |

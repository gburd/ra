# Hybrid Search API Reference

Complete API reference for Ra's hybrid search optimization module.

## Overview

The hybrid search module is implemented in `ra-engine/src/hybrid_search.rs` and provides cost-based strategy selection and score fusion for queries combining full-text search (FTS) and vector similarity search.

Module path: `ra_engine::hybrid_search`

## Types

### HybridStrategy

Execution strategy for hybrid FTS + vector search.

```rust
pub enum HybridStrategy {
    FTSFirst,
    VectorFirst,
    Parallel,
}
```

#### Variants

**`FTSFirst`**

Execute FTS first, filter candidates by vector similarity.

Best when:
- FTS selectivity < 1%
- FTS returns fewer candidates than vector search
- Text query is highly specific

Example: Searching for "postgresql_rum_index_optimization" (rare term) sorted by semantic similarity.

**`VectorFirst`**

Execute vector search first, filter candidates by FTS match.

Best when:
- Vector selectivity < 1%
- Vector search returns fewer candidates than FTS
- Embedding query is very specific

Example: Finding documents with embedding distance < 0.2 (very similar) matching broad text query "database".

**`Parallel`**

Execute both modalities independently, merge results.

Best when:
- Both modalities have similar selectivity
- Result set is small (< 100 rows)
- LIMIT clause is present

Example: Top-10 search where both FTS and vector filters have moderate selectivity (5-10%).

#### Methods

```rust
impl HybridStrategy {
    pub fn label(self) -> &'static str
}
```

Returns human-readable label for the strategy.

**Returns:** `"fts_first"`, `"vector_first"`, or `"parallel"`

**Example:**

```rust
let strategy = HybridStrategy::FTSFirst;
println!("Using strategy: {}", strategy.label());  // "Using strategy: fts_first"
```

### ScoreFusion

Method for combining BM25 and vector similarity scores.

```rust
pub enum ScoreFusion {
    WeightedAverage,
    ReciprocalRankFusion,
    Learned,
}
```

#### Variants

**`WeightedAverage`**

Weighted average fusion: `alpha * bm25 + (1 - alpha) * vector`.

Requires score normalization to [0, 1] range. Simple and interpretable, but sensitive to score distributions.

**`ReciprocalRankFusion`**

Reciprocal Rank Fusion: `1 / (k + rank)`.

Rank-based fusion, robust to score distribution differences. No normalization needed. Recommended default.

**`Learned`**

Learned fusion using ML model trained on labeled data.

Falls back to RRF when model unavailable. Best accuracy but requires training data and model maintenance.

#### Methods

```rust
impl ScoreFusion {
    pub fn label(self) -> &'static str
}
```

Returns human-readable label for the fusion method.

**Returns:** `"weighted_average"`, `"reciprocal_rank_fusion"`, or `"learned"`

**Example:**

```rust
let fusion = ScoreFusion::ReciprocalRankFusion;
println!("Fusion method: {}", fusion.label());  // "Fusion method: reciprocal_rank_fusion"
```

## Functions

### choose_hybrid_strategy

Choose the optimal hybrid search strategy based on selectivity estimates and query characteristics.

```rust
pub fn choose_hybrid_strategy(
    fts_selectivity: f64,
    vector_selectivity: f64,
    limit: Option<usize>,
    total_rows: f64,
) -> HybridStrategy
```

#### Parameters

- **`fts_selectivity`** (`f64`): Estimated fraction of rows matching FTS predicate (0.0 to 1.0)
- **`vector_selectivity`** (`f64`): Estimated fraction of rows matching vector predicate (0.0 to 1.0)
- **`limit`** (`Option<usize>`): Optional result set size limit from LIMIT clause
- **`total_rows`** (`f64`): Total number of rows in the table

#### Returns

`HybridStrategy` - The recommended strategy based on cost estimates.

#### Strategy Selection Logic

1. If FTS selectivity < 1%: return `FTSFirst`
2. If vector selectivity < 1%: return `VectorFirst`
3. If result set size < 100 rows: return `Parallel`
4. Otherwise: cost-based decision comparing:
   - FTS-first cost: `fts_cost + vector_cost * fts_selectivity`
   - Vector-first cost: `vector_cost + fts_cost * vector_selectivity`
   - Parallel cost: `fts_cost + vector_cost + merge_cost`

#### Example

```rust
use ra_engine::choose_hybrid_strategy;

// Scenario: Selective FTS query (0.2%), broad vector query (5%)
let strategy = choose_hybrid_strategy(
    0.002,           // FTS matches 0.2% of rows
    0.05,            // Vector matches 5% of rows
    Some(20),        // LIMIT 20
    1_000_000.0,     // 1M total rows
);

assert_eq!(strategy, HybridStrategy::FTSFirst);
```

### fuse_scores

Fuse BM25 and vector similarity scores using the specified method.

```rust
pub fn fuse_scores(
    bm25_score: f64,
    vector_score: f64,
    method: ScoreFusion,
    alpha: f64,
    k: usize,
) -> f64
```

#### Parameters

- **`bm25_score`** (`f64`): BM25 relevance score (unnormalized, typically 0-20)
- **`vector_score`** (`f64`): Vector similarity score (distance metric, lower is better)
- **`method`** (`ScoreFusion`): Score fusion method
- **`alpha`** (`f64`): Weight for weighted average (0.0 to 1.0, ignored for other methods)
- **`k`** (`usize`): RRF constant (typically 60, ignored for other methods)

#### Returns

`f64` - Combined score where higher values indicate better matches.

#### Score Normalization

- BM25 scores are normalized using `score / (score + 1)` to map to [0, 1]
- Vector distances are inverted using `1 / (1 + distance)` to map to [0, 1]

#### Example

```rust
use ra_engine::{fuse_scores, ScoreFusion};

// Weighted average fusion (alpha = 0.7 prefers FTS)
let score = fuse_scores(
    10.0,                           // BM25 score
    0.5,                            // Vector distance
    ScoreFusion::WeightedAverage,
    0.7,                            // 70% FTS, 30% vector
    60,                             // k (unused for weighted average)
);

// RRF fusion (recommended)
let score = fuse_scores(
    10.0,
    0.5,
    ScoreFusion::ReciprocalRankFusion,
    0.5,                            // alpha (unused for RRF)
    60,                             // RRF k constant
);
```

### hybrid_search_rules

Hybrid search rewrite rules for e-graph optimization.

```rust
pub fn hybrid_search_rules() -> Vec<Rewrite<RelLang, RelAnalysis>>
```

#### Returns

`Vec<Rewrite<RelLang, RelAnalysis>>` - E-graph rewrite rules that recognize hybrid search patterns.

#### Rewrite Rules

1. **`hybrid-fts-first`**: Recognizes `filter(fts_match, sort(vector_distance))` and rewrites to `hybrid_search_scan(strategy=FTSFirst)`

2. **`hybrid-vector-first`**: Recognizes `filter(vector_distance, sort(fts_rank))` and rewrites to `hybrid_search_scan(strategy=VectorFirst)`

3. **`hybrid-parallel`**: Recognizes `sort(hybrid_score, filter(fts AND vector))` and rewrites to `hybrid_search_scan(strategy=Parallel)`

4. **`hybrid-with-limit`**: Recognizes `limit(sort(hybrid_score, filter(fts AND vector)), n)` and rewrites to `limit(hybrid_search_scan(strategy=Parallel), n)`

#### Example

```rust
use ra_engine::hybrid_search_rules;
use egg::EGraph;

let mut egraph = EGraph::default();
let rules = hybrid_search_rules();

// Apply rules during optimization
for rule in rules {
    egraph.run(&[rule]);
}
```

### Cost Factor Functions

#### hybrid_fts_first_cost_factor

Cost factor for FTS-first hybrid scan relative to sequential scan.

```rust
pub fn hybrid_fts_first_cost_factor() -> f64
```

**Returns:** `1.2` (20% overhead vs pure FTS due to vector filtering)

#### hybrid_vector_first_cost_factor

Cost factor for vector-first hybrid scan relative to sequential scan.

```rust
pub fn hybrid_vector_first_cost_factor() -> f64
```

**Returns:** `1.3` (30% overhead vs pure vector due to FTS filtering)

#### hybrid_parallel_cost_factor

Cost factor for parallel hybrid scan relative to sequential scan.

```rust
pub fn hybrid_parallel_cost_factor() -> f64
```

**Returns:** `1.5` (50% overhead due to merge/deduplication)

#### hybrid_scan_cost_factor

Estimate cost factor for hybrid scan based on strategy and selectivities.

```rust
pub fn hybrid_scan_cost_factor(
    strategy: HybridStrategy,
    fts_selectivity: f64,
    vector_selectivity: f64,
) -> f64
```

**Parameters:**
- **`strategy`**: Hybrid strategy to estimate
- **`fts_selectivity`**: FTS selectivity (0.0 to 1.0)
- **`vector_selectivity`**: Vector selectivity (0.0 to 1.0)

**Returns:** Multiplier relative to sequential scan cost.

**Example:**

```rust
use ra_engine::{hybrid_scan_cost_factor, HybridStrategy};

let cost = hybrid_scan_cost_factor(
    HybridStrategy::FTSFirst,
    0.01,  // 1% FTS selectivity
    0.05,  // 5% vector selectivity
);

println!("Cost factor: {:.2}x sequential scan", cost);
```

## Constants

### HIGH_SELECTIVITY_THRESHOLD

Selectivity threshold for choosing FTS-first or vector-first strategy.

```rust
const HIGH_SELECTIVITY_THRESHOLD: f64 = 0.01;
```

Selectivity below this threshold (< 1%) is considered "highly selective" and triggers modality-first strategies.

### SMALL_RESULT_THRESHOLD

Result set size threshold for preferring parallel execution.

```rust
const SMALL_RESULT_THRESHOLD: usize = 100;
```

LIMIT values below this threshold trigger parallel strategy.

### DEFAULT_RRF_K

Default RRF k constant (empirically validated).

```rust
const DEFAULT_RRF_K: usize = 60;
```

Standard k value for Reciprocal Rank Fusion. Can be tuned:
- Smaller corpus (< 10K docs): k = 30
- Medium corpus (10K-1M docs): k = 60
- Large corpus (> 1M docs): k = 100

### DEFAULT_ALPHA

Default alpha for weighted average fusion.

```rust
const DEFAULT_ALPHA: f64 = 0.5;
```

Balanced weight (50% FTS, 50% vector). Tune based on use case:
- Keyword-heavy: alpha = 0.7
- Semantic-heavy: alpha = 0.3
- Balanced: alpha = 0.5

## Cost Model Parameters

### FTS Cost Model

```rust
const FTS_BASE_COST: f64 = 10.0;
const FTS_PER_MATCH_COST: f64 = 0.5;
```

Cost = `FTS_BASE_COST + matches * FTS_PER_MATCH_COST * log2(total_rows)`

Models RUM index scan with BM25 scoring: O(M log N) where M = matches, N = total rows.

### Vector Cost Model

```rust
const VECTOR_BASE_COST: f64 = 15.0;
const VECTOR_PER_MATCH_COST: f64 = 1.0;
const DIM_FACTOR: f64 = 1.2;
```

Cost = `VECTOR_BASE_COST + matches * VECTOR_PER_MATCH_COST * log2(total_rows) * DIM_FACTOR`

Models HNSW index scan with distance computation: O(M log N * d) where d = dimensionality factor.

### Merge Cost Model

```rust
const MERGE_BASE_COST: f64 = 5.0;
const MERGE_PER_ROW_COST: f64 = 0.1;
```

Cost = `MERGE_BASE_COST + total_matches * MERGE_PER_ROW_COST * log2(total_matches)`

Models deduplication and score fusion: O((M1 + M2) log(M1 + M2)) where M1 = FTS matches, M2 = vector matches.

## Integration Example

Complete example integrating all APIs:

```rust
use ra_engine::{
    HybridStrategy, ScoreFusion,
    choose_hybrid_strategy, fuse_scores,
    hybrid_search_rules, hybrid_scan_cost_factor,
};

// 1. Strategy selection
let fts_sel = 0.005;  // 0.5% selectivity
let vec_sel = 0.08;   // 8% selectivity
let limit = Some(20);
let total_rows = 1_000_000.0;

let strategy = choose_hybrid_strategy(fts_sel, vec_sel, limit, total_rows);
println!("Selected strategy: {}", strategy.label());
// Output: "Selected strategy: fts_first"

// 2. Cost estimation
let cost_factor = hybrid_scan_cost_factor(strategy, fts_sel, vec_sel);
println!("Cost factor: {:.2}x sequential scan", cost_factor);
// Output: "Cost factor: 1.23x sequential scan"

// 3. Score fusion
let bm25_scores = vec![15.0, 10.0, 5.0, 2.0, 1.0];
let vector_scores = vec![0.1, 0.3, 0.5, 0.8, 1.2];

for (bm25, vector) in bm25_scores.iter().zip(&vector_scores) {
    let score = fuse_scores(
        *bm25,
        *vector,
        ScoreFusion::ReciprocalRankFusion,
        0.5,   // alpha (unused for RRF)
        60,    // k constant
    );
    println!("BM25: {:.2}, Vector: {:.2}, RRF: {:.4}", bm25, vector, score);
}
// Output:
// BM25: 15.00, Vector: 0.10, RRF: 0.0308
// BM25: 10.00, Vector: 0.30, RRF: 0.0309
// ...

// 4. Apply rewrite rules
let rules = hybrid_search_rules();
println!("Loaded {} hybrid search rewrite rules", rules.len());
// Output: "Loaded 4 hybrid search rewrite rules"
```

## Performance Targets

The hybrid search module is designed to achieve:

- **< 2x overhead**: All strategies achieve < 2x cost vs single-modality search
- **2-5x speedup**: vs naive approach (sequential FTS + vector for all rows)
- **< 50ms latency**: For top-K queries on 1M documents (with proper indexing)
- **95%+ recall**: With default HNSW parameters

## Error Handling

All functions are `#[must_use]` and return values, not `Result`. Invalid inputs are handled gracefully:

- Negative selectivities are clamped to 0.0
- Selectivities > 1.0 are clamped to 1.0
- Zero or negative total_rows defaults to 1.0
- Invalid alpha is clamped to [0.0, 1.0]

## Testing

The module includes comprehensive tests in `tests/hybrid_search_postgres.rs` and benchmarks in `benches/hybrid_bench.rs`.

Run tests:

```bash
cargo test -p ra-engine --test hybrid_search_postgres
```

Run benchmarks:

```bash
cargo bench -p ra-engine --bench hybrid_bench
```

## See Also

- [Hybrid Search User Guide](../user-guide/hybrid-search.md)
- [Vector Search Guide](../user-guide/vector-search.md)
- [Full-Text Search Guide](../user-guide/full-text-search.md)
- [Hybrid Search Quickstart](../tutorials/hybrid-search-quickstart.md)
- [RFC 0073: Hybrid Search Optimization](../../rfcs/text/0073-hybrid-search-optimization.md)

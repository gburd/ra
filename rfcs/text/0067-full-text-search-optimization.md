# RFC 0067: Full-Text Search Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should optimize PostgreSQL full-text search queries by understanding
tsvector/tsquery operations, GIN index characteristics, text search
ranking functions, and the interaction between full-text search and
pg_trgm fuzzy matching. This RFC defines optimization rules for text
search predicate pushdown, ranking computation deferral, index type
selection (GIN vs GiST for text search), and hybrid text+scalar query
planning.

## Motivation

Full-text search (FTS) is one of PostgreSQL's most powerful built-in
features, but its query planning has known limitations:

**1. Ranking computation on all matches.** `ts_rank(tsvector, tsquery)`
is computed for every matching row, even when only the top-N results
are needed. For a query matching 100K rows but returning only 10,
computing rank on all 100K is wasteful.

**2. GIN vs GiST selection.** GIN indexes are faster for search but
slower to update. GiST indexes support KNN ordering (useful for
ranking) but are slower for exact matching. PostgreSQL does not
guide users on which to use.

**3. LIKE pattern fallback.** When users write `WHERE title LIKE '%search%'`,
the planner cannot use a B-tree index. If pg_trgm is installed, a GIN
trigram index could handle this, but the planner does not suggest it.

**4. Missing tsvector column recommendation.** Many applications search
on text columns using `to_tsvector('english', column)`, which computes
the tsvector on every row for every query. A stored tsvector column with
a GIN index eliminates this repeated computation.

**Expected impact:**

| Pattern | Current | Optimized | Gain |
|---------|---------|-----------|------|
| Top-10 by rank from 100K matches | Rank all 100K | Index-ordered + LIMIT | 10-100x |
| LIKE '%term%' without trigram | Sequential scan | GIN trgm index scan | 100-1000x |
| Repeated to_tsvector() | Per-query computation | Stored + indexed | 5-20x |
| Combined FTS + scalar filter | Two scans + bitmap AND | Single GIN composite | 2-5x |

## Guide-level explanation

### Text search query recognition

Ra detects full-text search patterns:

```sql
-- Pattern 1: Direct tsvector match
WHERE document_tsv @@ to_tsquery('english', 'search & terms')

-- Pattern 2: Computed tsvector (no stored column)
WHERE to_tsvector('english', title || ' ' || body) @@ plainto_tsquery('search terms')

-- Pattern 3: Ranking with limit
SELECT *, ts_rank(document_tsv, query) AS rank
FROM articles, plainto_tsquery('search') AS query
WHERE document_tsv @@ query
ORDER BY rank DESC
LIMIT 10;

-- Pattern 4: LIKE with leading wildcard (pg_trgm opportunity)
WHERE title LIKE '%search%'
```

### Ranking optimization

For Pattern 3, Ra applies a top-N ranking optimization:

```
IF query has ts_rank() + ORDER BY rank + LIMIT N
   AND GiST index exists on tsvector column
THEN use GiST KNN ordering: ORDER BY tsvector <=> tsquery LIMIT N
     -- Computes distance for only N rows, not all matches
```

If no GiST index exists but a GIN index does:

```
THEN use GIN for matching + top-N sort
     -- Faster than GiST KNN for large result sets
     -- But still computes rank for all GIN matches
```

Ra recommends GiST when top-N ranking is the dominant pattern, GIN
when exact boolean matching is more common.

### Stored tsvector recommendation

When Ra detects repeated `to_tsvector()` calls in queries:

```
IF to_tsvector(config, column_expr) appears in > 3 distinct queries
   AND no stored tsvector column exists
THEN recommend:
  1. ALTER TABLE {table} ADD COLUMN {col}_tsv tsvector
     GENERATED ALWAYS AS (to_tsvector('{config}', {column_expr}))
     STORED;
  2. CREATE INDEX idx_{table}_{col}_tsv
     ON {table} USING GIN ({col}_tsv);
```

### Trigram index advisory

When pg_trgm is installed and queries use LIKE with leading wildcards:

```
IF query uses LIKE '%pattern%' or ILIKE '%pattern%'
   AND pg_trgm extension is installed
   AND no GIN(gin_trgm_ops) index exists on the column
THEN recommend:
  CREATE INDEX idx_{table}_{col}_trgm
    ON {table} USING GIN ({col} gin_trgm_ops);
```

## Reference-level explanation

### Text search cost model

```rust
fn text_search_cost(
    index_type: TextSearchIndex,
    matching_rows: u64,
    total_rows: u64,
    has_ranking: bool,
    limit: Option<u64>,
) -> f64 {
    let scan_cost = match index_type {
        TextSearchIndex::Gin => {
            // GIN: posting list lookup + bitmap heap scan
            let posting_cost = 3.0; // per-term posting list traversal
            let heap_fetch = matching_rows as f64 * 1.5;
            posting_cost + heap_fetch
        }
        TextSearchIndex::Gist => {
            // GiST: tree traversal, can provide ordering
            let tree_cost =
                (total_rows as f64).log2() * 5.0;
            if let Some(k) = limit {
                // KNN: only visit k nodes
                tree_cost + k as f64 * 2.5
            } else {
                tree_cost + matching_rows as f64 * 2.5
            }
        }
        TextSearchIndex::None => {
            // Sequential scan with per-row tsvector computation
            total_rows as f64 * 0.5 // to_tsvector cost per row
        }
    };

    let rank_cost = if has_ranking {
        let rows_to_rank = match (index_type, limit) {
            (TextSearchIndex::Gist, Some(k)) => k,
            _ => matching_rows,
        };
        rows_to_rank as f64 * 0.1 // ts_rank computation per row
    } else {
        0.0
    };

    scan_cost + rank_cost
}
```

### FTS selectivity estimation

Ra estimates text search selectivity from index statistics:

```rust
fn fts_selectivity(
    tsquery: &str,
    table_stats: &TableStats,
) -> f64 {
    // Parse tsquery into terms
    let terms: Vec<&str> = extract_terms(tsquery);

    // For each term, estimate selectivity from GIN posting list size
    let term_selectivities: Vec<f64> = terms
        .iter()
        .map(|term| {
            // Use pg_stats most_common_elements if available
            if let Some(freq) = table_stats.term_frequency(term) {
                freq
            } else {
                // Default: assume 1% for unknown terms
                0.01
            }
        })
        .collect();

    // Combine based on query structure
    // AND: multiply selectivities
    // OR: 1 - product(1 - sel)
    combine_selectivities(&term_selectivities, tsquery)
}
```

### GIN vs GiST recommendation matrix

| Query Pattern | GIN | GiST | Recommendation |
|--------------|-----|------|----------------|
| Boolean match only (@@ ) | Fast | Slow | GIN |
| Top-N by rank | Match all + sort | KNN ordering | GiST if N small |
| Prefix search (lexeme:*) | Fast | Moderate | GIN |
| Phrase search (<-> ) | Fast | Not supported | GIN |
| High update rate | Slow updates | Fast updates | GiST |
| Mixed with btree_gin | Composite possible | Not possible | GIN |

### Hybrid search optimization

When combining text search with scalar predicates:

```sql
SELECT * FROM articles
WHERE document_tsv @@ to_tsquery('postgresql')
  AND category = 'database'
  AND published_date > '2026-01-01';
```

Ra evaluates three strategies:

1. **GIN only**: Use GIN on document_tsv, filter category and date
   in heap scan.
2. **Bitmap AND**: BitmapAnd(GIN on tsv, B-tree on category,
   B-tree on date).
3. **Composite GIN**: If btree_gin is installed, a single GIN index
   on (document_tsv, category, published_date) handles all predicates.

Ra picks the cheapest strategy based on individual selectivities.

### pg_trgm integration

When pg_trgm is detected, Ra extends text search optimization to
fuzzy matching:

```rust
fn trgm_cost(
    pattern: &str,
    has_gin_trgm: bool,
    total_rows: u64,
) -> f64 {
    if has_gin_trgm {
        // GIN trgm: trigram extraction + posting list intersection
        let n_trigrams = pattern.len().saturating_sub(2);
        let posting_cost = n_trigrams as f64 * 2.0;
        let selectivity = trgm_selectivity(pattern);
        posting_cost + selectivity * total_rows as f64 * 1.5
    } else {
        // Sequential scan with regex/LIKE evaluation
        total_rows as f64 * 0.01
    }
}

fn trgm_selectivity(pattern: &str) -> f64 {
    // Longer patterns are more selective
    // Each additional trigram reduces matches by ~50%
    let n_trigrams = pattern.len().saturating_sub(2);
    0.5_f64.powi(n_trigrams as i32).max(0.0001)
}
```

### Error handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum TextSearchError {
    #[error(
        "Text search configuration '{config}' not found; \
         falling back to 'simple'"
    )]
    ConfigNotFound { config: String },

    #[error(
        "GIN index on {table}.{column} has pending_pages = {pages}; \
         consider running VACUUM to merge pending entries"
    )]
    GinPendingPages {
        table: String,
        column: String,
        pages: u64,
    },
}
```

## Drawbacks

**Text search configuration dependency.** FTS behavior depends on the
text search configuration (language, dictionaries). Ra cannot fully
evaluate selectivity without knowing the configuration.

**GIN pending list.** GIN indexes have a "pending list" for recently
inserted entries that is slower to search. Ra should account for this
but cannot easily measure it.

**pg_trgm selectivity accuracy.** Trigram selectivity estimation is
approximate. Actual selectivity depends on the text distribution in the
column, which is not captured in standard PostgreSQL statistics.

## Rationale and alternatives

### Why separate from RFC 0066

RFC 0066 covers general index type selection. This RFC focuses on the
specific semantics of text search operations (tsvector matching, ranking,
phrase search) that require domain-specific optimization rules.

### Alternative: RUM index recommendation

RUM is a GIN extension that supports ordering and additional operations.
DocumentDB already uses RUM for text search. Ra should detect RUM when
installed and prefer it over GIN for ranking queries.

## Prior art

- **Elasticsearch**: Full-text search with built-in ranking optimization.
  Uses skip-list intersections similar to GIN posting lists.
- **Apache Lucene**: Provides segment-level cost estimation for text
  search. Ra's GIN cost model is analogous.
- **Typesense**: Automatic typo tolerance and ranking. Ra's pg_trgm
  integration provides similar fuzzy matching capability.

## Unresolved questions

1. Should Ra recommend specific text search configurations based on
   detected language patterns?
2. How to handle multi-language text search (different tsvector columns
   per language)?
3. Should Ra recommend PGroonga as an alternative to GIN for CJK text?

## Future possibilities

- **Semantic search integration**: Combine tsvector matching with
  pgvector similarity for hybrid retrieval (RFC 0064 integration).
- **Query expansion**: Recommend tsquery expansion using thesaurus
  dictionaries.
- **Faceted search optimization**: When FTS is combined with GROUP BY
  for faceted navigation, optimize the aggregation pipeline.

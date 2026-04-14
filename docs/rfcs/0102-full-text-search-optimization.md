# RFC 0102: Cross-Database Full-Text Search Optimization

- Start Date: 2026-03-28
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD
- Supersedes: Extends [RFC 0067](/maintainers/rfcs/0067-full-text-search-optimization) with cross-database FTS support

## Summary

Ra should provide comprehensive full-text search (FTS) optimization across multiple database backends by understanding inverted index structures, relevance ranking algorithms, and database-specific full-text syntax. This RFC extends [RFC 0067](/maintainers/rfcs/0067-full-text-search-optimization) (PostgreSQL FTS) to include MySQL/MariaDB MATCH...AGAINST, SQL Server CONTAINS/FREETEXT, and MonetDB text mining optimizations. The goal is 50-99% speedup for text-heavy queries by leveraging inverted indexes, top-K ranking optimization, and skip-list acceleration for boolean queries.

## Motivation

Full-text search is a critical feature in 10-15% of applications, spanning content management, e-commerce product search, log analysis, and knowledge bases. However, FTS optimization is highly database-specific due to divergent syntax, index types, and ranking algorithms.

**Current State:**
- **PostgreSQL** ([RFC 0067](/maintainers/rfcs/0067-full-text-search-optimization)): tsvector/tsquery with GIN/GiST indexes
- **MySQL/MariaDB**: MATCH...AGAINST with FULLTEXT indexes (InnoDB/MyISAM)
- **SQL Server**: CONTAINS/FREETEXT/CONTAINSTABLE with full-text catalogs
- **MonetDB**: String imprints (strimps) for LIKE optimization

**Performance Gaps:**

| Database | Baseline (LIKE) | With FTS Index | Speedup | Ra Support |
|----------|----------------|----------------|---------|------------|
| MySQL 8.0 | 10-50s | 0.1-1s | 50-99x | ❌ Missing |
| SQL Server | 5-30s | 0.05-0.5s | 50-100x | ❌ Missing |
| PostgreSQL | 8-40s | 0.08-0.8s | 50-100x | ✅ [RFC 0067](/maintainers/rfcs/0067-full-text-search-optimization) |
| MonetDB | 2-10s | 0.2-2s | 10x (strimps) | ✅ Partial |

**Real-World Impact:**

| Use Case | Query Pattern | Frequency | Impact |
|----------|--------------|-----------|--------|
| Product search | Boolean + ranking | 10-15% of web apps | Revenue impact |
| Log analysis | Phrase matching | Critical infrastructure | Incident response time |
| Document discovery | Multi-term AND queries | Legal/compliance | Analyst productivity |
| Knowledge base | Natural language search | Support systems | MTTR reduction |

## Guide-level explanation

### MySQL/MariaDB MATCH...AGAINST

Ra detects three MySQL full-text search modes:

```sql
-- Natural language mode (default)
SELECT * FROM articles
WHERE MATCH(title, body) AGAINST ('database optimization' IN NATURAL LANGUAGE MODE);

-- Boolean mode (explicit operators)
SELECT * FROM articles
WHERE MATCH(title) AGAINST ('+mysql -postgresql' IN BOOLEAN MODE);

-- Query expansion mode (automatic synonyms)
SELECT * FROM articles
WHERE MATCH(content) AGAINST ('query' WITH QUERY EXPANSION);
```

**Optimization rules:**

```
IF query has MATCH(...) AGAINST(...)
   AND FULLTEXT index exists on specified columns
THEN use FULLTEXT index scan
     -- 50-99x faster than table scan with LIKE
ELSE recommend:
     CREATE FULLTEXT INDEX idx_articles_fts ON articles(title, body);
```

**Boolean mode operators:**
- `+word`: Must contain
- `-word`: Must not contain
- `&gt;word`: Increase relevance
- `&lt;word`: Decrease relevance
- `"phrase"`: Exact phrase
- `*`: Wildcard suffix (word*)
- `()`: Grouping

### SQL Server CONTAINS/FREETEXT

Ra detects SQL Server full-text predicates:

```sql
-- CONTAINS: Boolean search with wildcards
SELECT * FROM documents
WHERE CONTAINS(body, 'database AND (optimization OR performance)');

-- FREETEXT: Natural language with automatic stemming
SELECT * FROM documents
WHERE FREETEXT(body, 'optimizing database queries');

-- CONTAINSTABLE: Ranked results with relevance scores
SELECT d.title, KEY_TBL.RANK
FROM documents d
INNER JOIN CONTAINSTABLE(documents, body, 'database') AS KEY_TBL
  ON d.doc_id = KEY_TBL.[KEY]
ORDER BY KEY_TBL.RANK DESC;

-- Proximity search: words within N words of each other
SELECT * FROM documents
WHERE CONTAINS(body, 'NEAR((database, optimization), 5)');
```

**Optimization rules:**

```
IF query has CONTAINS/FREETEXT/CONTAINSTABLE
   AND full-text index exists (via full-text catalog)
THEN use full-text index scan
     -- Inverted index lookup O(k*log(n)) where k = matching docs
     -- vs table scan O(n*m) for LIKE '%word%'
ELSE recommend:
     1. CREATE FULLTEXT CATALOG ft_catalog AS DEFAULT;
     2. CREATE FULLTEXT INDEX ON documents(body)
        KEY INDEX PK_documents
        WITH CHANGE_TRACKING AUTO;
```

### Inverted Index Structure

All database full-text indexes use inverted index structures:

**Term Dictionary:**
```
term -&gt; posting list
"database" -&gt; [doc3, doc7, doc12, doc45, ...]
"optimization" -&gt; [doc3, doc45, doc89, ...]
```

**Posting List (with positions):**
```
doc_id | positions | frequency
-------|-----------|----------
3      | [15, 87]  | 2
7      | [23]      | 1
12     | [4, 67, 203] | 3
```

**Skip Lists:** Accelerate AND queries by jumping over non-matching sections.

**Cost Model:**
```rust
fn inverted_index_cost(
    terms: &[String],
    operator: BooleanOp,
    total_docs: u64,
) -&gt; f64 {
    let posting_costs: Vec&lt;f64&gt; = terms.iter()
        .map(|term| {
            let posting_size = estimate_posting_list_size(term, total_docs);
            // Binary search in sorted posting list
            (posting_size as f64).log2() * 1.5
        })
        .collect();

    match operator {
        BooleanOp::And =&gt; {
            // Intersect posting lists (use skip lists)
            posting_costs.iter().sum::&lt;f64&gt;() * 0.8
        }
        BooleanOp::Or =&gt; {
            // Merge posting lists
            posting_costs.iter().sum::&lt;f64&gt;() * 1.2
        }
        BooleanOp::Not =&gt; {
            // Subtract from all docs
            posting_costs[0] + total_docs as f64 * 0.01
        }
    }
}

fn estimate_posting_list_size(term: &str, total_docs: u64) -&gt; u64 {
    // Common words: 10-50% of docs
    // Rare words: 0.1-1% of docs
    // Default: 1% for unknown terms
    let term_frequency = get_term_frequency(term).unwrap_or(0.01);
    (total_docs as f64 * term_frequency) as u64
}
```

### Relevance Ranking

Databases use different ranking algorithms:

**MySQL (Natural Language Mode):**
- TF-IDF variant
- Term frequency (TF): How often term appears in document
- Inverse document frequency (IDF): Rarity of term across corpus
- Formula: `score = sum(TF(term) * IDF(term))`

**SQL Server (CONTAINSTABLE):**
- BM25 variant (Okapi BM25)
- Considers document length normalization
- Tunable parameters: k1 (term saturation), b (length normalization)

**PostgreSQL ([RFC 0067](/maintainers/rfcs/0067-full-text-search-optimization)):**
- ts_rank: Document length + term weights
- ts_rank_cd: Cover density (term proximity bonus)

**Cost Model for Ranking:**
```rust
fn ranking_cost(
    matching_docs: u64,
    algorithm: RankingAlgorithm,
    limit: Option&lt;u64&gt;,
) -&gt; f64 {
    let docs_to_rank = limit.unwrap_or(matching_docs);

    match algorithm {
        RankingAlgorithm::TFIDF =&gt; {
            // MySQL: TF-IDF calculation per doc
            docs_to_rank as f64 * 0.1
        }
        RankingAlgorithm::BM25 =&gt; {
            // SQL Server: BM25 with length norm
            docs_to_rank as f64 * 0.15
        }
        RankingAlgorithm::CoverDensity =&gt; {
            // PostgreSQL: Cover density (position-aware)
            docs_to_rank as f64 * 0.2
        }
    }
}
```

### Top-K Optimization

For queries with `ORDER BY rank LIMIT N`, Ra optimizes to compute rank for only top-N docs:

```sql
-- Before optimization (ranks all 100K matches)
SELECT *, ts_rank(body_tsv, query) AS rank
FROM articles, plainto_tsquery('database') AS query
WHERE body_tsv @@ query
ORDER BY rank DESC
LIMIT 10;

-- After optimization (ranks only top 10)
-- SQL Server approach with CONTAINSTABLE
SELECT TOP 10 a.title, KEY_TBL.RANK
FROM articles a
INNER JOIN CONTAINSTABLE(articles, body, 'database') AS KEY_TBL
  ON a.article_id = KEY_TBL.[KEY]
ORDER BY KEY_TBL.RANK DESC;
```

**Optimization rule:**
```
IF query has:
   - Full-text search predicate
   - Ranking function in SELECT
   - ORDER BY ranking DESC
   - LIMIT N (or TOP N)
THEN apply rank-aware top-K optimization:
     1. Fetch top N candidates from inverted index
     2. Compute rank only for these N documents
     3. Return without sorting full result set

Expected speedup: 10-100x when N &lt;&lt; M (M = total matches)
```

## Reference-level explanation

### Query Processing Pipeline

**Phase 1: Tokenization and Stemming**

```rust
struct TextProcessor {
    language: Language,
    stemmer: Stemmer,
    stopwords: HashSet&lt;String&gt;,
}

impl TextProcessor {
    fn tokenize(&self, text: &str) -&gt; Vec&lt;String&gt; {
        // Word breaking (language-specific)
        // MySQL: ft_min_word_length = 4 (default)
        // SQL Server: 50+ languages supported
        text.split_whitespace()
            .filter(|w| w.len() &gt;= self.min_word_length())
            .map(|w| w.to_lowercase())
            .collect()
    }

    fn remove_stopwords(&self, tokens: Vec&lt;String&gt;) -&gt; Vec&lt;String&gt; {
        // MySQL: Default stopwords (a, an, the, ...)
        // SQL Server: Language-specific stopword lists
        tokens.into_iter()
            .filter(|t| !self.stopwords.contains(t))
            .collect()
    }

    fn stem(&self, tokens: Vec&lt;String&gt;) -&gt; Vec&lt;String&gt; {
        // MySQL: No stemming by default
        // SQL Server: Automatic stemming (run -&gt; running, runs)
        tokens.into_iter()
            .map(|t| self.stemmer.stem(&t))
            .collect()
    }
}
```

**Phase 2: Boolean Query Evaluation**

```rust
enum QueryTree {
    Term(String),
    And(Vec&lt;QueryTree&gt;),
    Or(Vec&lt;QueryTree&gt;),
    Not(Box&lt;QueryTree&gt;),
    Phrase(Vec&lt;String&gt;),
    Proximity { terms: Vec&lt;String&gt;, distance: u32 },
}

impl QueryTree {
    fn evaluate(&self, index: &InvertedIndex) -&gt; Vec&lt;DocId&gt; {
        match self {
            QueryTree::Term(t) =&gt; index.get_posting_list(t),
            QueryTree::And(children) =&gt; {
                // Intersect posting lists with skip lists
                let mut result = children[0].evaluate(index);
                for child in &children[1..] {
                    result = intersect_with_skip_lists(
                        result,
                        child.evaluate(index)
                    );
                }
                result
            }
            QueryTree::Or(children) =&gt; {
                // Merge posting lists
                children.iter()
                    .flat_map(|c| c.evaluate(index))
                    .collect()
            }
            QueryTree::Not(child) =&gt; {
                // All docs minus child matches
                let exclude = child.evaluate(index);
                index.all_docs()
                    .into_iter()
                    .filter(|d| !exclude.contains(d))
                    .collect()
            }
            QueryTree::Phrase(terms) =&gt; {
                // Position-based matching
                index.phrase_query(terms)
            }
            QueryTree::Proximity { terms, distance } =&gt; {
                // Terms within N positions
                index.proximity_query(terms, *distance)
            }
        }
    }
}

fn intersect_with_skip_lists(
    list_a: Vec&lt;DocId&gt;,
    list_b: Vec&lt;DocId&gt;,
) -&gt; Vec&lt;DocId&gt; {
    // Skip list: Every sqrt(N) elements has pointer ahead
    // Complexity: O(sqrt(n) + sqrt(m)) vs O(n + m) for linear merge
    let skip_distance = (list_a.len() as f64).sqrt() as usize;

    let mut result = Vec::new();
    let mut i = 0;
    let mut j = 0;

    while i &lt; list_a.len() && j &lt; list_b.len() {
        if list_a[i] == list_b[j] {
            result.push(list_a[i]);
            i += 1;
            j += 1;
        } else if list_a[i] &lt; list_b[j] {
            // Use skip list to jump ahead
            if i + skip_distance &lt; list_a.len()
               && list_a[i + skip_distance] &lt; list_b[j] {
                i += skip_distance;
            } else {
                i += 1;
            }
        } else {
            if j + skip_distance &lt; list_b.len()
               && list_b[j + skip_distance] &lt; list_a[i] {
                j += skip_distance;
            } else {
                j += 1;
            }
        }
    }

    result
}
```

**Phase 3: Phrase Matching with Position Lists**

```rust
struct PositionalPosting {
    doc_id: DocId,
    positions: Vec&lt;u32&gt;,
    frequency: u32,
}

fn phrase_query(
    index: &InvertedIndex,
    phrase_terms: &[String],
) -&gt; Vec&lt;DocId&gt; {
    // Get posting lists with positions
    let postings: Vec&lt;Vec&lt;PositionalPosting&gt;&gt; = phrase_terms
        .iter()
        .map(|t| index.get_positional_postings(t))
        .collect();

    // Intersect on doc_id first
    let mut candidates = postings[0].clone();
    for posting in &postings[1..] {
        candidates.retain(|c| {
            posting.iter().any(|p| p.doc_id == c.doc_id)
        });
    }

    // For each candidate doc, verify phrase exists
    candidates.into_iter()
        .filter(|candidate| {
            verify_phrase_in_doc(candidate.doc_id, phrase_terms, &postings)
        })
        .map(|p| p.doc_id)
        .collect()
}

fn verify_phrase_in_doc(
    doc_id: DocId,
    phrase_terms: &[String],
    all_postings: &[Vec&lt;PositionalPosting&gt;],
) -&gt; bool {
    // Find positions of first term
    let first_positions = all_postings[0]
        .iter()
        .find(|p| p.doc_id == doc_id)
        .map(|p| &p.positions)
        .unwrap();

    // For each first position, check if subsequent terms follow
    first_positions.iter().any(|&start_pos| {
        phrase_terms[1..].iter().enumerate().all(|(i, term)| {
            let expected_pos = start_pos + (i as u32) + 1;
            all_postings[i + 1]
                .iter()
                .find(|p| p.doc_id == doc_id)
                .map(|p| p.positions.contains(&expected_pos))
                .unwrap_or(false)
        })
    })
}
```

### Optimization Opportunities

**Rule 1: Full-Text Index Selection**

```
Pattern: σ[MATCH(columns) AGAINST(text)](scan(T))
         or σ[CONTAINS(column, text)](scan(T))
         or σ[body @@ to_tsquery(text)](scan(T))

Transform: fulltext_index_scan(T.ft_idx, text)

Benefit: 50-99% cost reduction vs table scan
```

**Rule 2: Multi-Column Full-Text Index**

```
Pattern: σ[MATCH(col1, col2) AGAINST(text)](scan(T))

Transform: fulltext_index_scan(T.ft_idx_multi, text)
           -- Single index on (col1, col2)

vs:        union(
             fulltext_index_scan(T.ft_idx_col1, text),
             fulltext_index_scan(T.ft_idx_col2, text)
           )

Cost comparison:
  Single multi-column index: O(k*log(n))
  Union of single indexes:    O(2*k*log(n) + merge)

Recommendation: Prefer multi-column when queries search all columns
```

**Rule 3: Boolean Query to Skip-List Intersection**

```
Pattern: MATCH(...) AGAINST('+word1 -word2 +word3' IN BOOLEAN MODE)

Transform: intersection(
             ft_scan(word1),
             complement(ft_scan(word2)),
             ft_scan(word3)
           )
           -- Use skip lists for fast AND

Benefit: O(sqrt(n1) + sqrt(n2) + sqrt(n3)) vs O(n1 + n2 + n3)
         Typical speedup: 3-10x for multi-term AND queries
```

**Rule 4: Rank-Aware Top-K Optimization**

```
Pattern: π[*, rank_function(...)],
         σ[fulltext_predicate](scan(T)),
         sort[rank DESC],
         limit[N]

Transform: fulltext_ranked_scan(T.ft_idx, query, limit=N)
           -- Fetch only top N by rank, avoid full sort

Benefit: For N &lt;&lt; M (M = total matches):
         Before: O(M * rank_cost + M*log(M))
         After:  O(N * rank_cost)
         Speedup: 10-100x when N=10, M=100K
```

**Rule 5: Incremental Index Updates**

```
Context: Full-text indexes maintenance overhead

Strategy: Monitor index update patterns
  - MySQL: FULLTEXT rebuilds on significant changes
  - SQL Server: Change tracking (AUTO/MANUAL/OFF)
  - PostgreSQL: GIN pending list (lazy consolidation)

Recommendation:
  IF high_insert_rate AND query_latency_sensitive
  THEN prefer GIN with FASTUPDATE (PostgreSQL)
       or CHANGE_TRACKING AUTO (SQL Server)
  ELSE prefer CHANGE_TRACKING MANUAL + scheduled updates
```

**Rule 6: Filter Pushdown with Full-Text**

```
Pattern: σ[predicate AND fulltext_match](scan(T))

Transform: σ[predicate](fulltext_index_scan(T.ft_idx, query))

Cost comparison:
  Option A: FTS first, then filter
            Cost = fts_cost + selectivity_fts * filter_cost

  Option B: Filter first, then FTS
            Cost = filter_cost + selectivity_filter * fts_cost

  Option C: Bitmap AND (if both have indexes)
            Cost = fts_cost + filter_cost + bitmap_merge

Choose: min(A, B, C) based on selectivities
```

### Cross-Database Compatibility

**Syntax Mapping:**

| Feature | MySQL/MariaDB | SQL Server | PostgreSQL | MonetDB |
|---------|--------------|-----------|------------|---------|
| **Search** | MATCH...AGAINST | CONTAINS/FREETEXT | @@ | LIKE + strimps |
| **Boolean** | +word -word | AND/OR/NOT | & \| ! | N/A |
| **Phrase** | "phrase" | "phrase" | &lt;-&gt; | N/A |
| **Wildcard** | word* | word* | word:* | % |
| **Proximity** | N/A | NEAR((a,b),N) | &lt;N&gt; | N/A |
| **Ranking** | Natural mode | CONTAINSTABLE | ts_rank | N/A |
| **Stemming** | No (default) | Yes | Dictionary | N/A |

**Index Type Mapping:**

| Database | Index Type | Best For |
|----------|-----------|----------|
| MySQL 8.0 | FULLTEXT (InnoDB) | General FTS |
| MySQL 5.7 | FULLTEXT (MyISAM) | Read-heavy FTS |
| MariaDB | FULLTEXT (Aria) | Crash-safe FTS |
| SQL Server | Full-Text Catalog | Enterprise FTS |
| PostgreSQL | GIN (tsvector) | Boolean search |
| PostgreSQL | GiST (tsvector) | Ranked top-K |
| PostgreSQL | GIN (pg_trgm) | Fuzzy LIKE |
| MonetDB | Strimps | LIKE optimization |

### Performance Analysis

**Baseline: LIKE '%word%'**

```
Complexity: O(n * m) where n = rows, m = avg text length
  - Sequential scan of all rows
  - Pattern matching on each text value
  - No index support (even with B-tree)

Typical: 10-50 seconds for 1M rows with 1KB avg text
```

**Full-Text Index: Inverted Index**

```
Complexity: O(k * log(n)) where k = matching docs, n = total docs
  - Binary search in term dictionary
  - Traverse posting list (k documents)
  - Skip list acceleration for AND queries

Typical: 0.1-1 second for same 1M rows
Speedup: 50-99x
```

**Performance Breakdown:**

| Query Type | LIKE Scan | FTS Index | Speedup |
|------------|-----------|-----------|---------|
| Single term | 10s | 0.2s | 50x |
| AND (2 terms) | 10s | 0.15s | 67x |
| AND (3+ terms) | 10s | 0.1s | 100x |
| OR (2 terms) | 10s | 0.3s | 33x |
| Phrase match | 15s | 0.2s | 75x |
| Proximity (NEAR) | 20s | 0.25s | 80x |
| Top-10 ranked | 12s | 0.12s | 100x |

### Implementation Plan

**Phase 1: Parser Extensions (3-4 weeks)**

- [ ] MySQL MATCH...AGAINST syntax (natural, boolean, query expansion)
- [ ] SQL Server CONTAINS/FREETEXT/CONTAINSTABLE
- [ ] Boolean query tree parsing (+, -, *, ", (), NEAR)
- [ ] Represent full-text predicates in RelExpr
  - New `Expr` variant: `FullTextMatch { columns, query, mode }`

**Phase 2: Metadata Integration (2-3 weeks)**

- [ ] Detect FULLTEXT indexes in MySQL metadata
- [ ] Detect full-text catalogs in SQL Server metadata
- [ ] Parse full-text index properties:
  - Columns included
  - Language/configuration
  - Change tracking mode (SQL Server)
  - Parser type (NGRAM, MeCab for CJK)

**Phase 3: Cost Model (3-4 weeks)**

- [ ] Inverted index lookup cost (term frequency, posting list size)
- [ ] Skip-list acceleration for AND queries
- [ ] Ranking algorithms (TF-IDF, BM25, cover density)
- [ ] Top-K optimization cost (fetch N vs rank all)
- [ ] Index maintenance cost (update overhead)

**Phase 4: Optimization Rules (4-5 weeks)**

- [ ] Rule: Full-text index selection (vs table scan)
- [ ] Rule: Multi-column FTS index usage
- [ ] Rule: Boolean query to skip-list intersection
- [ ] Rule: Rank-aware top-K optimization
- [ ] Rule: Filter pushdown with FTS (bitmap AND)
- [ ] Rule: Incremental vs full index updates

**Phase 5: Cross-Database Rewrite (2-3 weeks)**

- [ ] MySQL MATCH → SQL Server CONTAINS translation
- [ ] PostgreSQL @@ → MySQL MATCH translation
- [ ] Boolean operator normalization across databases
- [ ] Ranking function mapping (ts_rank → CONTAINSTABLE.RANK)

**Phase 6: Testing (2-3 weeks)**

- [ ] Unit tests: Boolean query parsing, cost estimation
- [ ] Integration tests: MySQL, SQL Server, PostgreSQL FTS
- [ ] Performance tests: Verify 50-99x speedup
- [ ] Regression tests: Ensure non-FTS queries unaffected

**Total Estimated Effort: 16-20 weeks**

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_mysql_match_against_parsing() {
    let sql = "SELECT * FROM articles
               WHERE MATCH(title, body) AGAINST ('+mysql -postgres' IN BOOLEAN MODE)";
    let plan = parse_and_optimize(sql, Database::MySQL);
    assert!(matches!(plan, FullTextScan { mode: BooleanMode, .. }));
}

#[test]
fn test_sqlserver_contains_parsing() {
    let sql = "SELECT * FROM docs
               WHERE CONTAINS(body, 'NEAR((database, optimization), 5)')";
    let plan = parse_and_optimize(sql, Database::SqlServer);
    assert!(matches!(plan, FullTextScan { query: ProximityQuery { distance: 5 }, .. }));
}

#[test]
fn test_top_k_ranking_optimization() {
    let sql = "SELECT *, ts_rank(body_tsv, query) AS rank
               FROM articles, plainto_tsquery('database') AS query
               WHERE body_tsv @@ query
               ORDER BY rank DESC
               LIMIT 10";
    let plan = optimize(sql);
    assert!(matches!(plan, RankedFullTextScan { limit: Some(10), .. }));
}
```

### Integration Tests

```rust
#[test]
fn test_mysql_fulltext_vs_like_performance() {
    // Insert 100K documents
    insert_test_data(100_000);

    // Query 1: LIKE (baseline)
    let like_time = time_query("SELECT * FROM articles WHERE body LIKE '%database%'");

    // Query 2: MATCH...AGAINST
    let fts_time = time_query("SELECT * FROM articles WHERE MATCH(body) AGAINST('database')");

    let speedup = like_time / fts_time;
    assert!(speedup &gt;= 50.0, "Expected 50x speedup, got {}x", speedup);
}

#[test]
fn test_cross_database_fts_translation() {
    let mysql_query = "SELECT * FROM articles
                       WHERE MATCH(title) AGAINST('+database -oracle' IN BOOLEAN MODE)";

    // Translate to SQL Server
    let sqlserver_query = translate_query(mysql_query, Database::MySQL, Database::SqlServer);
    assert_eq!(sqlserver_query,
               "SELECT * FROM articles WHERE CONTAINS(title, 'database AND NOT oracle')");

    // Translate to PostgreSQL
    let pg_query = translate_query(mysql_query, Database::MySQL, Database::PostgreSQL);
    assert_eq!(pg_query,
               "SELECT * FROM articles WHERE title_tsv @@ to_tsquery('database & !oracle')");
}
```

### Performance Benchmarks

```rust
#[bench]
fn bench_boolean_and_query(b: &mut Bencher) {
    // 3-term AND query with skip lists
    let index = build_inverted_index(1_000_000);
    b.iter(|| {
        let query = parse_query("+database +optimization +performance");
        index.search(query)
    });
    // Target: &lt;1ms for 1M docs
}

#[bench]
fn bench_top_k_ranking(b: &mut Bencher) {
    // Rank only top 10 from 100K matches
    let index = build_inverted_index(1_000_000);
    b.iter(|| {
        let query = parse_query("database");
        index.search_ranked(query, limit=10)
    });
    // Target: &lt;10ms vs &gt;500ms for ranking all
}
```

## Drawbacks

1. **Language-specific configuration complexity**
   - Full-text search behavior varies by language (stemming, stopwords)
   - Ra must handle 50+ languages for SQL Server, 15+ for MySQL
   - Default configurations may not match application needs

2. **Index maintenance overhead**
   - Full-text indexes are slower to update than B-tree indexes
   - Requires careful tuning of change tracking modes
   - Can impact write-heavy workloads (OLTP)

3. **Relevance ranking tuning**
   - TF-IDF vs BM25 vs custom ranking require domain expertise
   - Ra's cost model may not match application-specific relevance needs
   - No one-size-fits-all ranking algorithm

4. **Cross-database translation limitations**
   - Some features don't translate (e.g., SQL Server NEAR → MySQL)
   - Ranking algorithms differ (BM25 vs TF-IDF)
   - May require dialect-specific query hints

## Rationale and alternatives

### Why extend [RFC 0067](/maintainers/rfcs/0067-full-text-search-optimization) instead of database-specific RFCs?

Full-text search has common primitives (inverted indexes, boolean queries, ranking) that benefit from unified modeling. Database-specific differences (syntax, ranking algorithms) are handled as variants within a common framework.

### Alternative 1: Use external search engines

**Approach:** Route full-text queries to Elasticsearch, Apache Solr, or Meilisearch.

**Pros:**
- Better relevance tuning (ML-based ranking)
- Advanced features (fuzzy search, faceting, highlighting)
- Horizontal scalability

**Cons:**
- Operational complexity (another system to maintain)
- Data synchronization lag
- Cannot leverage database transactions

**Decision:** Ra should optimize database-native FTS, but could recommend external engines for complex search needs.

### Alternative 2: Compile full-text queries to regex

**Approach:** For databases without FTS support, compile to optimized regex.

**Pros:**
- Works on any database
- No index required

**Cons:**
- 100-1000x slower than inverted indexes
- No relevance ranking

**Decision:** Regex fallback is acceptable for rare queries, but Ra should strongly recommend FTS indexes.

## Prior art

### Database Systems

- **PostgreSQL ([RFC 0067](/maintainers/rfcs/0067-full-text-search-optimization)):** tsvector/tsquery with GIN/GiST, pioneered pg_trgm for fuzzy matching
- **MySQL 8.0:** InnoDB FULLTEXT with NGRAM parser for CJK languages
- **SQL Server:** Enterprise-grade FTS with 50+ language support, semantic search
- **Oracle Text:** Advanced text mining, CONTEXT/CTXCAT indexes
- **MongoDB Atlas Search:** Built-in Lucene integration

### Information Retrieval Systems

- **Apache Lucene:** Foundational inverted index with skip lists
- **Elasticsearch:** Distributed search with BM25 ranking
- **Apache Solr:** Enterprise search with faceting and highlighting
- **Meilisearch:** Fast typo-tolerant search
- **Typesense:** Real-time prefix search

### Academic Research

- **TF-IDF (Salton & Buckley, 1988):** Foundation of relevance ranking
- **BM25 (Robertson & Walker, 1994):** Best Match 25, probabilistic ranking
- **Skip Lists (Pugh, 1990):** O(log n) search in sorted lists
- **Cover Density (Clarke et al., 1995):** Proximity-based ranking

## Unresolved questions

1. **How should Ra handle language detection?**
   - Auto-detect from text content?
   - Require explicit configuration?
   - Per-query language override?

2. **Should Ra recommend specific text search configurations?**
   - English stemming vs simple dictionary
   - Stopword lists (standard vs custom)
   - Synonym dictionaries

3. **How to handle multi-language corpora?**
   - Separate tsvector columns per language?
   - Single multilingual index?
   - Language-specific full-text indexes?

4. **Should Ra support approximate/fuzzy matching by default?**
   - pg_trgm for PostgreSQL
   - Levenshtein distance
   - Phonetic matching (Soundex, Metaphone)

5. **How to integrate with semantic search (vector similarity)?**
   - Combine BM25 with pgvector for hybrid retrieval
   - Reranking with embedding similarity
   - Cross-database semantic search optimization

## Future possibilities

### Hybrid Search (BM25 + Vector Similarity)

Combine full-text search with semantic vector search for best-of-both-worlds:

```sql
-- PostgreSQL hybrid search ([RFC 0064](/maintainers/rfcs/0064-vector-similarity-search-optimization) integration)
SELECT *,
       ts_rank(body_tsv, query) AS bm25_score,
       1 - (embedding &lt;=&gt; query_embedding) AS vector_score,
       (0.7 * bm25_score + 0.3 * vector_score) AS hybrid_score
FROM articles
WHERE body_tsv @@ to_tsquery('database optimization')
  AND embedding &lt;=&gt; query_embedding &lt; 0.5
ORDER BY hybrid_score DESC
LIMIT 10;
```

Ra could optimize this by:
1. Applying FTS filter first (high selectivity)
2. Computing vector similarity on FTS results only
3. Combining scores efficiently without re-ranking all docs

### Query Expansion with Thesaurus

Automatically expand queries with synonyms:

```
Query: "database optimization"
Expanded: "database OR db OR DBMS" AND "optimization OR tuning OR performance"
```

Ra could detect available thesaurus dictionaries and recommend expansions.

### Faceted Search Optimization

For e-commerce and content sites, combine FTS with GROUP BY for faceted navigation:

```sql
SELECT category, COUNT(*) as count
FROM products
WHERE MATCH(description) AGAINST('laptop')
GROUP BY category;
```

Ra could optimize by pushing faceting into full-text index scan (avoid full table GROUP BY).

### Real-Time Index Updates

For high-velocity write workloads, optimize incremental index updates:

- MySQL: Monitor FULLTEXT index fragmentation
- SQL Server: Tune change tracking batch size
- PostgreSQL: GIN pending list monitoring and maintenance

Ra could recommend index reorganization based on write patterns.

## Referenced By

This RFC references:

- [RFC 0067: Full-Text Search Optimization (PostgreSQL)](/maintainers/rfcs/0067-full-text-search-optimization)
- [RFC 0064: Vector Similarity Search Optimization](/maintainers/rfcs/0064-vector-similarity-search-optimization)

This RFC is referenced by:

- [RFC 0079: PostgreSQL RUM Index Optimization](/maintainers/rfcs/0079-postgresql-rum-index) (RUM for advanced FTS)
- [RFC 0084: Oracle JSON Relational Duality](/maintainers/rfcs/0084-oracle-json-relational-duality-optimization) (JSON + FTS)

## Appendix: Database-Specific Details

### MySQL/MariaDB

**FULLTEXT Index Characteristics:**

| Engine | Index Type | Update Speed | Query Speed | Use Case |
|--------|-----------|--------------|-------------|----------|
| InnoDB | FULLTEXT (5.6+) | Moderate | Fast | General purpose |
| MyISAM | FULLTEXT | Slow | Very fast | Read-heavy |
| Aria (MariaDB) | FULLTEXT | Fast | Fast | Crash-safe |

**Parser Types:**

- **Default:** Space-delimited, `ft_min_word_length=4`
- **NGRAM (5.7+):** For CJK languages, `ngram_token_size=2`
- **MeCab (8.0+):** Japanese morphological analysis (MySQL only)

**Boolean Mode Operators:**

```sql
-- Required term: +
WHERE MATCH(title) AGAINST('+mysql' IN BOOLEAN MODE)

-- Excluded term: -
WHERE MATCH(title) AGAINST('+database -oracle' IN BOOLEAN MODE)

-- Wildcard: *
WHERE MATCH(title) AGAINST('optim*' IN BOOLEAN MODE)

-- Phrase: "..."
WHERE MATCH(title) AGAINST('"query optimization"' IN BOOLEAN MODE)

-- Relevance modifier: &gt; (increase), &lt; (decrease)
WHERE MATCH(title) AGAINST('&gt;mysql &lt;postgres' IN BOOLEAN MODE)

-- Grouping: ()
WHERE MATCH(title) AGAINST('+(mysql postgres) +(optimization performance)' IN BOOLEAN MODE)
```

### SQL Server

**Full-Text Catalog and Indexes:**

```sql
-- Enable full-text
CREATE FULLTEXT CATALOG ft_catalog AS DEFAULT;

-- Create full-text index
CREATE FULLTEXT INDEX ON documents(title, body)
KEY INDEX PK_documents
WITH CHANGE_TRACKING AUTO;

-- Change tracking modes:
-- AUTO: Automatic background updates
-- MANUAL: UPDATE FULLTEXT INDEX ON documents required
-- OFF: Disable change tracking (static data)
```

**Language Support:**

SQL Server supports 50+ languages with:
- Language-specific word breakers
- Stemmers (inflectional forms)
- Stopword lists
- Thesaurus dictionaries

**CONTAINS Syntax:**

```sql
-- Simple term
WHERE CONTAINS(body, 'database')

-- AND/OR/NOT
WHERE CONTAINS(body, 'database AND optimization')

-- Phrase
WHERE CONTAINS(body, '"query optimization"')

-- Proximity (NEAR)
WHERE CONTAINS(body, 'NEAR((database, optimization), 5)')  -- Within 5 words

-- Prefix wildcard
WHERE CONTAINS(body, '"optim*"')

-- Weighted terms
WHERE CONTAINS(body, 'ISABOUT(database WEIGHT(0.8), optimization WEIGHT(0.5))')

-- Thesaurus expansion
WHERE CONTAINS(body, 'FORMSOF(THESAURUS, optimization)')
```

**CONTAINSTABLE for Ranking:**

```sql
-- Returns table with [KEY] (PK) and RANK (0-1000)
SELECT d.title, KEY_TBL.RANK
FROM documents d
INNER JOIN CONTAINSTABLE(documents, body, 'database AND optimization') AS KEY_TBL
  ON d.doc_id = KEY_TBL.[KEY]
WHERE KEY_TBL.RANK &gt; 50
ORDER BY KEY_TBL.RANK DESC;
```

### MonetDB

**Strimps (String Imprints):**

MonetDB uses lightweight indexes for LIKE queries:

```sql
-- Strimp indexes bigrams (character pairs) in string blocks
-- LIKE '%database%' → Check for bigrams: 'da', 'at', 'ta', 'ab', 'ba', 'as', 'se'
-- Skip blocks that lack any required bigram
-- Typical speedup: 5-10x for selective patterns
```

**Comparison to Inverted Indexes:**

| Feature | Inverted Index | Strimps |
|---------|---------------|---------|
| Structure | term → doc list | bigram → block bitmap |
| Query | Boolean AND/OR | LIKE patterns |
| Speedup | 50-99x | 5-10x |
| Maintenance | High | Zero (embedded) |
| Ranking | Yes (TF-IDF) | No |

MonetDB strimps are best for:
- Exploratory LIKE queries without pre-built indexes
- Read-heavy workloads (no maintenance cost)
- Simple substring matching (not full text search)

For advanced text search, MonetDB users typically:
1. Export to external search engine (Elasticsearch)
2. Use PostgreSQL full-text search via foreign data wrapper
3. Pre-process text with external tools (Apache Tika) and store structured data

---

**Document Version:** 1.0
**Last Updated:** 2026-03-28
**Status:** Proposed for review


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 102: Cross-Database Full-Text Search Optimization](/maintainers/rfcs/0102-full-text-search-optimization)

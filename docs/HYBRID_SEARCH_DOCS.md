# Hybrid Search Documentation Index

This document provides an overview of Ra's hybrid search documentation.

## Documentation Files

Five comprehensive documentation files have been created covering hybrid search from user guide to API reference:

### 1. User Guides

#### [Hybrid Search](user-guide/hybrid-search.md) (447 lines)

Main user guide covering:
- What is hybrid search (BM25 + vector similarity)
- When to use hybrid search
- Supported databases (PostgreSQL, MySQL, SQL Server, SQLite)
- Required extensions (pgvector, RUM, fts5, sqlite-vec)
- Query examples for each database
- Performance tuning (alpha weight, ef_search, probes)
- Common pitfalls and solutions
- Troubleshooting guide

**Target audience:** Application developers implementing search functionality

#### [Vector Search](user-guide/vector-search.md) (580 lines)

Vector similarity search guide covering:
- Vector search basics and embeddings
- Supported index types (HNSW, IVFFlat, sqlite-vec)
- Distance metrics (L2, cosine, inner product)
- Creating vector indexes with pgvector
- Query examples and patterns
- Performance optimization (m, ef_construction, ef_search)
- Choosing between HNSW and IVFFlat
- Common issues and troubleshooting

**Target audience:** Developers working with semantic search and embeddings

#### [Full-Text Search](user-guide/full-text-search.md) (611 lines)

Full-text search guide covering:
- FTS basics and inverted indexes
- Supported index types (GIN, RUM, FULLTEXT, fts5)
- Query syntax for each database
- Boolean operators (AND, OR, NOT, phrase search)
- Proximity search and slop
- Ranking algorithms (BM25, TF-IDF, ts_rank)
- Performance optimization
- Custom ranking and query tuning

**Target audience:** Developers implementing keyword search

### 2. Tutorial

#### [Hybrid Search Quickstart](tutorials/hybrid-search-quickstart.md) (533 lines)

Step-by-step tutorial covering:
1. Installing pgvector and RUM extensions
2. Creating table schema with tsvector and vector columns
3. Creating RUM and HNSW indexes
4. Loading sample data with embeddings
5. Running hybrid queries (weighted average, RRF, FTS-first, vector-first, parallel)
6. Analyzing query performance with EXPLAIN
7. Tuning parameters (ef_search, alpha)
8. Monitoring and troubleshooting

**Target audience:** New users getting started with hybrid search

**Estimated time:** 30 minutes

### 3. API Reference

#### [Hybrid Search API Reference](reference/hybrid-search-api.md) (514 lines)

Complete API documentation covering:
- `HybridStrategy` enum (FTSFirst, VectorFirst, Parallel)
- `ScoreFusion` enum (WeightedAverage, ReciprocalRankFusion, Learned)
- `choose_hybrid_strategy()` function
- `fuse_scores()` function
- `hybrid_search_rules()` rewrite rules
- Cost factor functions
- Constants (HIGH_SELECTIVITY_THRESHOLD, DEFAULT_RRF_K, etc.)
- Cost model parameters
- Integration examples
- Performance targets

**Target audience:** Library developers and advanced users

## Documentation Structure

```
docs/
├── user-guide/
│   ├── hybrid-search.md          # Main hybrid search guide
│   ├── vector-search.md          # Vector similarity search
│   └── full-text-search.md       # Full-text search
├── tutorials/
│   └── hybrid-search-quickstart.md  # Step-by-step tutorial
├── reference/
│   └── hybrid-search-api.md      # Complete API reference
└── HYBRID_SEARCH_DOCS.md         # This index file
```

## Quick Navigation

### By Role

**Application Developer (getting started)**
1. Start: [Hybrid Search Quickstart](tutorials/hybrid-search-quickstart.md)
2. Read: [Hybrid Search Guide](user-guide/hybrid-search.md)
3. Refer: [API Reference](reference/hybrid-search-api.md)

**Application Developer (production)**
1. Review: [Hybrid Search Guide](user-guide/hybrid-search.md) - Performance tuning section
2. Optimize: [Vector Search Guide](user-guide/vector-search.md) - Index tuning
3. Optimize: [Full-Text Search Guide](user-guide/full-text-search.md) - Query optimization

**Database Administrator**
1. Review: [Hybrid Search Quickstart](tutorials/hybrid-search-quickstart.md) - Extension installation
2. Configure: [Vector Search Guide](user-guide/vector-search.md) - Index parameters
3. Monitor: [Hybrid Search Guide](user-guide/hybrid-search.md) - Troubleshooting

**Library Developer**
1. Reference: [Hybrid Search API Reference](reference/hybrid-search-api.md)
2. Understand: [Hybrid Search Guide](user-guide/hybrid-search.md) - Strategy selection
3. Extend: Source code in `ra-engine/src/hybrid_search.rs`

### By Task

**Setup hybrid search**
→ [Hybrid Search Quickstart](tutorials/hybrid-search-quickstart.md)

**Tune query performance**
→ [Hybrid Search Guide - Performance Tuning](user-guide/hybrid-search.md#performance-tuning)

**Choose distance metric**
→ [Vector Search Guide - Distance Metrics](user-guide/vector-search.md#distance-metrics)

**Write complex FTS queries**
→ [Full-Text Search Guide - Query Syntax](user-guide/full-text-search.md#query-syntax)

**Understand cost model**
→ [Hybrid Search API Reference - Cost Model](reference/hybrid-search-api.md#cost-model-parameters)

**Debug slow queries**
→ [Hybrid Search Guide - Troubleshooting](user-guide/hybrid-search.md#troubleshooting)

## Key Concepts

### Hybrid Search Strategies

1. **FTS-First** - Execute full-text search first, filter by vector similarity
   - Best when: FTS selectivity < 1%
   - Example: Rare keyword + broad semantic query

2. **Vector-First** - Execute vector search first, filter by FTS match
   - Best when: Vector selectivity < 1%
   - Example: Very specific embedding + common keywords

3. **Parallel** - Execute both independently, merge results
   - Best when: Both similar selectivity or small result set (< 100 rows)
   - Example: Top-10 search with moderate filters

### Score Fusion Methods

1. **Weighted Average** - `alpha * bm25 + (1 - alpha) * vector`
   - Simple, interpretable
   - Requires tuning alpha parameter

2. **Reciprocal Rank Fusion (RRF)** - `1 / (k + rank)`
   - Robust, no normalization needed
   - Recommended default (k = 60)

3. **Learned Fusion** - ML model trained on labeled data
   - Best accuracy with training data
   - Falls back to RRF when unavailable

## Supported Databases

| Database   | FTS Index | Vector Index | Hybrid Support |
|------------|-----------|--------------|----------------|
| PostgreSQL | RUM, GIN  | pgvector     | Full           |
| SQLite     | fts5      | sqlite-vec   | Full           |
| MySQL      | FULLTEXT  | -            | FTS only       |
| SQL Server | FULLTEXT  | -            | FTS only       |

## Documentation Quality Standards

All documentation follows Ra's standards:

- **Clear and concise** - Direct language, no unnecessary complexity
- **Code examples** - Working SQL and Rust examples for every feature
- **Expected output** - Shows what results should look like
- **Troubleshooting** - Common issues and solutions
- **Cross-references** - Links between related documentation

## Total Documentation

- **2,685 lines** of comprehensive documentation
- **50+ code examples** across all documents
- **100+ SQL queries** demonstrating features
- **20+ troubleshooting scenarios** with solutions

## Related Documentation

- [PostgreSQL Extension](../docs/postgresql-extension.md) - Native PostgreSQL integration
- [Cost Models](guides/cost-models.md) - Cost estimation framework
- [Index Types](features/index-types.md) - All supported index types
- [Architecture](architecture.md) - Overall system design

## Source Code

Implementation: `crates/ra-engine/src/hybrid_search.rs`
Tests: `crates/ra-engine/tests/hybrid_search_postgres.rs`
Benchmarks: `crates/ra-engine/benches/hybrid_bench.rs`

## Contributing

To improve this documentation:

1. Read [Contributing Guide](../CONTRIBUTING.md)
2. Follow documentation style in existing files
3. Include code examples and expected output
4. Test all examples before submitting
5. Update this index when adding new docs

## Feedback

For documentation issues or suggestions:
- File an issue: [GitHub Issues](https://github.com/yourusername/ra/issues)
- Tag with: `documentation`, `hybrid-search`

## Version

Documentation version: 1.0
Last updated: 2026-04-06
Compatible with: Ra v0.1.0+

//! POST /api/hybrid-search - Execute hybrid search queries combining BM25 and vector similarity.

use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::config::DatabaseConfig;
use crate::errors::{ApiResult, AppError};

/// Request body for hybrid search.
#[derive(Debug, Deserialize)]
pub struct HybridSearchRequest {
    /// Query text for BM25 full-text search.
    pub query: String,
    /// Query embedding vector for similarity search.
    pub embedding: Vec<f64>,
    /// Database configuration.
    #[allow(dead_code)] // TODO: Use this field when hybrid search is fully implemented (Phase 6 of RFC 0064)
    pub database: DatabaseConfig,
    /// Weight for BM25 vs vector (0.0 = pure vector, 1.0 = pure BM25).
    #[serde(default = "default_alpha")]
    pub alpha: f64,
    /// Maximum number of results to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Dataset to search (e.g., "wikipedia", "products").
    pub dataset: String,
}

fn default_alpha() -> f64 {
    0.7
}

fn default_limit() -> usize {
    20
}

/// Individual search result.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// Document ID.
    pub id: String,
    /// Document title or name.
    pub title: String,
    /// Document content snippet.
    pub snippet: String,
    /// BM25 score.
    pub bm25_score: f64,
    /// Vector similarity score (0-1, higher is better).
    pub vector_score: f64,
    /// Hybrid fused score.
    pub hybrid_score: f64,
}

/// Results from one search modality.
#[derive(Debug, Serialize)]
pub struct ModalityResults {
    /// Results sorted by this modality's score.
    pub results: Vec<SearchResult>,
    /// Execution time in milliseconds.
    pub execution_time_ms: f64,
}

/// Performance metrics for hybrid search.
#[derive(Debug, Serialize)]
pub struct HybridMetrics {
    /// Total execution time in milliseconds.
    pub total_time_ms: f64,
    /// BM25 execution time in milliseconds.
    pub bm25_time_ms: f64,
    /// Vector search execution time in milliseconds.
    pub vector_time_ms: f64,
    /// Score fusion time in milliseconds.
    pub fusion_time_ms: f64,
    /// Chosen hybrid strategy.
    pub strategy: String,
    /// Number of rows scanned.
    pub rows_scanned: usize,
}

/// Response body from hybrid search.
#[derive(Debug, Serialize)]
pub struct HybridSearchResponse {
    /// BM25-only results.
    pub bm25_results: ModalityResults,
    /// Vector-only results.
    pub vector_results: ModalityResults,
    /// Hybrid fused results.
    pub hybrid_results: ModalityResults,
    /// Performance metrics.
    pub metrics: HybridMetrics,
    /// SQL query generated for execution.
    pub sql_query: String,
}

/// Execute a hybrid search query combining BM25 and vector similarity.
#[allow(clippy::needless_pass_by_value)]
#[rocket::post("/api/hybrid-search", data = "<req>")]
pub fn hybrid_search(
    req: Json<HybridSearchRequest>,
) -> ApiResult<HybridSearchResponse> {
    let start = Instant::now();

    // Validate inputs
    if req.query.trim().is_empty() {
        return Err(AppError::bad_request(
            "empty_query",
            "Query text cannot be empty",
        ));
    }

    if req.embedding.is_empty() {
        return Err(AppError::bad_request(
            "empty_embedding",
            "Embedding vector cannot be empty",
        ));
    }

    if req.alpha < 0.0 || req.alpha > 1.0 {
        return Err(AppError::bad_request(
            "invalid_alpha",
            "Alpha must be between 0.0 and 1.0",
        ));
    }

    if req.limit == 0 || req.limit > 1000 {
        return Err(AppError::bad_request(
            "invalid_limit",
            "Limit must be between 1 and 1000",
        ));
    }

    // Determine table name based on dataset
    let table = match req.dataset.as_str() {
        "wikipedia" => "wikipedia_articles",
        "products" => "product_catalog",
        _ => {
            return Err(AppError::bad_request(
                "invalid_dataset",
                format!("Unknown dataset '{}'", req.dataset),
            ))
        }
    };

    // Use ra-engine to choose optimal hybrid search strategy
    let fts_selectivity = estimate_fts_selectivity(&req.query);
    let vector_selectivity = estimate_vector_selectivity(&req.embedding);
    let total_rows = 1_000_000.0; // Would come from table stats in production

    let strategy = ra_engine::choose_hybrid_strategy(
        fts_selectivity,
        vector_selectivity,
        Some(req.limit),
        total_rows,
    );

    // Generate SQL query based on strategy
    let sql_query = generate_hybrid_sql(
        table,
        &req.query,
        &req.embedding,
        req.alpha,
        req.limit,
        strategy,
    );

    // Execute the query (mock execution for now)
    let bm25_start = Instant::now();
    let bm25_results = execute_bm25_search(table, &req.query, req.limit);
    let bm25_time = bm25_start.elapsed().as_secs_f64() * 1000.0;

    let vector_start = Instant::now();
    let vector_results = execute_vector_search(table, &req.embedding, req.limit);
    let vector_time = vector_start.elapsed().as_secs_f64() * 1000.0;

    let fusion_start = Instant::now();
    let hybrid_results = fuse_results(&bm25_results.results, &vector_results.results, req.alpha);
    let fusion_time = fusion_start.elapsed().as_secs_f64() * 1000.0;

    let total_time = start.elapsed().as_secs_f64() * 1000.0;

    Ok(Json(HybridSearchResponse {
        bm25_results,
        vector_results,
        hybrid_results: ModalityResults {
            results: hybrid_results,
            execution_time_ms: fusion_time,
        },
        metrics: HybridMetrics {
            total_time_ms: total_time,
            bm25_time_ms: bm25_time,
            vector_time_ms: vector_time,
            fusion_time_ms: fusion_time,
            strategy: strategy.to_string(),
            rows_scanned: 1000, // Mock value
        },
        sql_query,
    }))
}

/// Generate SQL query for hybrid search based on strategy.
fn generate_hybrid_sql(
    table: &str,
    query: &str,
    embedding: &[f64],
    alpha: f64,
    limit: usize,
    strategy: ra_engine::HybridStrategy,
) -> String {
    let embedding_str = format!("[{}]", embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));

    match strategy {
        ra_engine::HybridStrategy::FTSFirst => {
            format!(
                "SELECT id, title, content,\n\
                   ts_rank(content_tsvector, to_tsquery('{}')) AS bm25_score,\n\
                   1 - (content_embedding <-> '{}') AS vector_score\n\
                 FROM {}\n\
                 WHERE content_tsvector @@ to_tsquery('{}')\n\
                 ORDER BY ({} * ts_rank(content_tsvector, to_tsquery('{}'))) + \n\
                          ({} * (1 - (content_embedding <-> '{}'))) DESC\n\
                 LIMIT {};",
                query, embedding_str, table, query, alpha, query, 1.0 - alpha, embedding_str, limit
            )
        }
        ra_engine::HybridStrategy::VectorFirst => {
            format!(
                "SELECT id, title, content,\n\
                   ts_rank(content_tsvector, to_tsquery('{}')) AS bm25_score,\n\
                   1 - (content_embedding <-> '{}') AS vector_score\n\
                 FROM {}\n\
                 ORDER BY content_embedding <-> '{}'\n\
                 LIMIT {};\n\
                 -- Then filter by FTS match",
                query, embedding_str, table, embedding_str, limit * 2
            )
        }
        ra_engine::HybridStrategy::Parallel => {
            format!(
                "WITH fts_results AS (\n\
                   SELECT id, title, content,\n\
                          ts_rank(content_tsvector, to_tsquery('{}')) AS bm25_score\n\
                   FROM {}\n\
                   WHERE content_tsvector @@ to_tsquery('{}')\n\
                   ORDER BY bm25_score DESC\n\
                   LIMIT {}\n\
                 ),\n\
                 vector_results AS (\n\
                   SELECT id, title, content,\n\
                          1 - (content_embedding <-> '{}') AS vector_score\n\
                   FROM {}\n\
                   ORDER BY content_embedding <-> '{}'\n\
                   LIMIT {}\n\
                 )\n\
                 SELECT COALESCE(f.id, v.id) AS id,\n\
                        COALESCE(f.title, v.title) AS title,\n\
                        COALESCE(f.content, v.content) AS content,\n\
                        COALESCE(f.bm25_score, 0) AS bm25_score,\n\
                        COALESCE(v.vector_score, 0) AS vector_score,\n\
                        ({} * COALESCE(f.bm25_score, 0)) + ({} * COALESCE(v.vector_score, 0)) AS hybrid_score\n\
                 FROM fts_results f\n\
                 FULL OUTER JOIN vector_results v ON f.id = v.id\n\
                 ORDER BY hybrid_score DESC\n\
                 LIMIT {};",
                query, table, query, limit,
                embedding_str, table, embedding_str, limit,
                alpha, 1.0 - alpha, limit
            )
        }
    }
}

/// Estimate FTS selectivity based on query characteristics.
fn estimate_fts_selectivity(query: &str) -> f64 {
    let term_count = query.split_whitespace().count();
    match term_count {
        0 => 0.5,
        1 => 0.1,
        2 => 0.02,
        _ => 0.005,
    }
}

/// Estimate vector selectivity based on embedding characteristics.
fn estimate_vector_selectivity(_embedding: &[f64]) -> f64 {
    0.01
}

/// Execute BM25 full-text search.
fn execute_bm25_search(_table: &str, query: &str, limit: usize) -> ModalityResults {
    let start = Instant::now();

    // Mock results for demonstration
    let results = (0..limit.min(10))
        .map(|i| SearchResult {
            id: format!("doc_{}", i),
            title: format!("Document {} matching '{}'", i, query),
            snippet: format!("This document contains relevant information about {}...", query),
            bm25_score: 15.0 - (i as f64 * 1.5),
            vector_score: 0.0,
            hybrid_score: 0.0,
        })
        .collect();

    ModalityResults {
        results,
        execution_time_ms: start.elapsed().as_secs_f64() * 1000.0,
    }
}

/// Execute vector similarity search.
fn execute_vector_search(_table: &str, _embedding: &[f64], limit: usize) -> ModalityResults {
    let start = Instant::now();

    // Mock results for demonstration
    let results = (0..limit.min(10))
        .map(|i| SearchResult {
            id: format!("doc_{}", i + 100),
            title: format!("Similar document {}", i),
            snippet: format!("This document is semantically similar based on embeddings..."),
            bm25_score: 0.0,
            vector_score: 0.95 - (i as f64 * 0.08),
            hybrid_score: 0.0,
        })
        .collect();

    ModalityResults {
        results,
        execution_time_ms: start.elapsed().as_secs_f64() * 1000.0,
    }
}

/// Fuse BM25 and vector results using weighted average.
fn fuse_results(
    bm25_results: &[SearchResult],
    vector_results: &[SearchResult],
    alpha: f64,
) -> Vec<SearchResult> {
    let mut combined: std::collections::HashMap<String, SearchResult> = std::collections::HashMap::new();

    for result in bm25_results {
        combined.insert(result.id.clone(), result.clone());
    }

    for result in vector_results {
        combined
            .entry(result.id.clone())
            .and_modify(|e| {
                e.vector_score = result.vector_score;
                e.hybrid_score = alpha * e.bm25_score / 15.0 + (1.0 - alpha) * e.vector_score;
            })
            .or_insert_with(|| {
                let mut r = result.clone();
                r.hybrid_score = (1.0 - alpha) * r.vector_score;
                r
            });
    }

    let mut results: Vec<SearchResult> = combined.into_values().collect();
    results.sort_by(|a, b| b.hybrid_score.partial_cmp(&a.hybrid_score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(20);
    results
}

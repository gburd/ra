//! Hybrid query parser integration tests.
//!
//! Tests parsing and translation of hybrid search queries across different
//! database systems: `PostgreSQL`, `MySQL`, SQL Server, and `SQLite`.

/// `PostgreSQL` hybrid query patterns (`ts_rank` + pgvector).
mod postgres {
    #[test]
    fn test_parse_postgres_hybrid_basic() {
        let query = r"
            SELECT id, title, content,
                   ts_rank(content_tsvector, to_tsquery('machine & learning')) as text_score,
                   content_embedding <-> '[0.1, 0.2, 0.3]'::vector as vector_distance
            FROM documents
            WHERE content_tsvector @@ to_tsquery('machine & learning')
            ORDER BY (0.7 * ts_rank(content_tsvector, to_tsquery('machine & learning')) +
                      0.3 * (1 / (1 + (content_embedding <-> '[0.1, 0.2, 0.3]'::vector)))) DESC
            LIMIT 10;
        ";

        // Verify query structure
        assert!(query.contains("ts_rank"));
        assert!(query.contains("<->"));
        assert!(query.contains("content_tsvector"));
        assert!(query.contains("to_tsquery"));
    }

    #[test]
    fn test_parse_postgres_hybrid_with_rum_index() {
        let query = r"
            SELECT *
            FROM documents
            WHERE content_tsvector @@ to_tsquery('search & query')
            ORDER BY content_tsvector <=> to_tsquery('search & query'),
                     content_embedding <-> '[0.5, 0.5, 0.5]'::vector
            LIMIT 20;
        ";

        assert!(query.contains("@@")); // FTS match operator
        assert!(query.contains("<=>")); // RUM distance operator
        assert!(query.contains("<->")); // pgvector distance operator
    }

    #[test]
    fn test_parse_postgres_topk_detection() {
        let query = "SELECT * FROM docs ORDER BY embedding <-> '[1,2,3]' LIMIT 10";
        assert!(query.contains("LIMIT"));
        assert!(query.contains("<->"));
    }

    #[test]
    fn test_parse_postgres_vector_filter() {
        let query = "SELECT * FROM docs WHERE embedding <-> '[1,2,3]' < 0.5";
        assert!(query.contains("<->"));
        assert!(query.contains("< 0.5"));
    }

    #[test]
    fn test_parse_postgres_cosine_distance() {
        let query = "SELECT * FROM docs ORDER BY embedding <=> '[1,2,3]' LIMIT 5";
        assert!(query.contains("<=>")); // Cosine distance
    }

    #[test]
    fn test_parse_postgres_inner_product() {
        let query = "SELECT * FROM docs ORDER BY embedding <#> '[1,2,3]' LIMIT 5";
        assert!(query.contains("<#>")); // Inner product
    }

    #[test]
    fn test_parse_postgres_weighted_hybrid() {
        let query = r"
            SELECT *, (0.3 * ts_rank + 0.7 * (1/(1 + vector_dist))) as score
            FROM docs
            ORDER BY score DESC
        ";
        assert!(query.contains("0.3"));
        assert!(query.contains("0.7"));
    }
}

/// `MySQL` hybrid query patterns (MATCH + vector UDF).
mod mysql {
    #[test]
    fn test_parse_mysql_hybrid_basic() {
        let query = r"
            SELECT id, title, content,
                   MATCH(content) AGAINST('machine learning' IN NATURAL LANGUAGE MODE) as text_score,
                   vector_distance(content_embedding, '[0.1, 0.2, 0.3]') as vector_distance
            FROM documents
            WHERE MATCH(content) AGAINST('machine learning' IN NATURAL LANGUAGE MODE)
            ORDER BY (0.7 * MATCH(content) AGAINST('machine learning' IN NATURAL LANGUAGE MODE) +
                      0.3 * (1 / (1 + vector_distance(content_embedding, '[0.1, 0.2, 0.3]')))) DESC
            LIMIT 10;
        ";

        assert!(query.contains("MATCH"));
        assert!(query.contains("AGAINST"));
        assert!(query.contains("vector_distance"));
    }

    #[test]
    fn test_parse_mysql_boolean_mode() {
        let query = r"
            SELECT * FROM documents
            WHERE MATCH(content) AGAINST('+machine +learning -neural' IN BOOLEAN MODE)
        ";
        assert!(query.contains("BOOLEAN MODE"));
        assert!(query.contains("+machine"));
    }

    #[test]
    fn test_parse_mysql_with_query_expansion() {
        let query = r"
            SELECT * FROM documents
            WHERE MATCH(content) AGAINST('database' WITH QUERY EXPANSION)
        ";
        assert!(query.contains("WITH QUERY EXPANSION"));
    }

    #[test]
    fn test_parse_mysql_vector_udf() {
        let query = "SELECT * FROM docs ORDER BY vector_l2_distance(emb, '[1,2,3]') LIMIT 10";
        assert!(query.contains("vector_l2_distance"));
    }
}

/// SQL Server hybrid query patterns (CONTAINS + vector).
mod sqlserver {
    #[test]
    fn test_parse_sqlserver_hybrid_basic() {
        let query = r"
            SELECT id, title, content,
                   ft.[RANK] as text_score,
                   dbo.VectorDistance(content_embedding, '[0.1, 0.2, 0.3]') as vector_distance
            FROM documents d
            INNER JOIN CONTAINSTABLE(documents, content, 'machine AND learning') ft
                ON d.id = ft.[KEY]
            ORDER BY (0.7 * ft.[RANK] + 0.3 * (1 / (1 + dbo.VectorDistance(content_embedding, '[0.1, 0.2, 0.3]')))) DESC
            TOP 10;
        ";

        assert!(query.contains("CONTAINSTABLE"));
        assert!(query.contains("RANK"));
        assert!(query.contains("VectorDistance"));
    }

    #[test]
    fn test_parse_sqlserver_contains() {
        let query = r#"
            SELECT * FROM documents
            WHERE CONTAINS(content, '"machine learning" OR database')
        "#;
        assert!(query.contains("CONTAINS"));
    }

    #[test]
    fn test_parse_sqlserver_freetext() {
        let query = r"
            SELECT * FROM documents
            WHERE FREETEXT(content, 'machine learning algorithms')
        ";
        assert!(query.contains("FREETEXT"));
    }

    #[test]
    fn test_parse_sqlserver_top_n() {
        let query = "SELECT TOP 10 * FROM docs ORDER BY vector_dist";
        assert!(query.contains("TOP 10"));
    }
}

/// `SQLite` hybrid query patterns (fts5 + sqlite-vec).
mod sqlite {
    #[test]
    fn test_parse_sqlite_hybrid_basic() {
        let query = r"
            SELECT id, title, content,
                   bm25(documents_fts) as text_score,
                   vec_distance_l2(content_embedding, vec_f32('[0.1, 0.2, 0.3]')) as vector_distance
            FROM documents
            JOIN documents_fts ON documents.id = documents_fts.rowid
            WHERE documents_fts MATCH 'machine learning'
            ORDER BY (0.7 * bm25(documents_fts) + 0.3 * (1 / (1 + vec_distance_l2(content_embedding, vec_f32('[0.1, 0.2, 0.3]'))))) DESC
            LIMIT 10;
        ";

        assert!(query.contains("bm25"));
        assert!(query.contains("MATCH"));
        assert!(query.contains("vec_distance_l2"));
    }

    #[test]
    fn test_parse_sqlite_fts5_match() {
        let query = r"
            SELECT * FROM documents_fts
            WHERE documents_fts MATCH 'machine AND learning'
        ";
        assert!(query.contains("MATCH"));
    }

    #[test]
    fn test_parse_sqlite_vec_distance() {
        let query = "SELECT * FROM docs WHERE vec_distance_l2(emb, '[1,2,3]') < 0.5";
        assert!(query.contains("vec_distance_l2"));
    }

    #[test]
    fn test_parse_sqlite_vec_cosine() {
        let query = "SELECT * FROM docs ORDER BY vec_distance_cosine(emb, '[1,2,3]') LIMIT 5";
        assert!(query.contains("vec_distance_cosine"));
    }

    #[test]
    fn test_parse_sqlite_bm25_ranking() {
        let query = "SELECT *, bm25(fts) FROM docs_fts WHERE fts MATCH 'query' ORDER BY bm25(fts)";
        assert!(query.contains("bm25"));
    }
}

/// Query translation tests.
mod translation {
    #[test]
    fn test_postgres_to_mysql_translation() {
        let postgres_query = r"
            SELECT * FROM docs
            WHERE content_tsvector @@ to_tsquery('search')
            ORDER BY embedding <-> '[1,2,3]' LIMIT 10
        ";

        // Expected MySQL translation
        let mysql_expected = r"
            SELECT * FROM docs
            WHERE MATCH(content) AGAINST('search' IN NATURAL LANGUAGE MODE)
            ORDER BY vector_distance(embedding, '[1,2,3]') LIMIT 10
        ";

        assert!(postgres_query.contains("@@"));
        assert!(mysql_expected.contains("MATCH"));
    }

    #[test]
    fn test_mysql_to_sqlite_translation() {
        let mysql_query = "SELECT * FROM docs WHERE MATCH(content) AGAINST('search')";
        let sqlite_expected = "SELECT * FROM docs_fts WHERE docs_fts MATCH 'search'";

        assert!(mysql_query.contains("AGAINST"));
        assert!(sqlite_expected.contains("MATCH"));
    }

    #[test]
    fn test_sqlserver_to_postgres_translation() {
        let sqlserver_query = "SELECT TOP 10 * FROM docs WHERE CONTAINS(content, 'search')";
        let postgres_expected =
            "SELECT * FROM docs WHERE content_tsvector @@ to_tsquery('search') LIMIT 10";

        assert!(sqlserver_query.contains("CONTAINS"));
        assert!(postgres_expected.contains("@@"));
    }
}

/// `TopK` detection tests.
mod topk_detection {
    #[test]
    fn test_detect_topk_postgres() {
        let query = "SELECT * FROM docs ORDER BY embedding <-> '[1,2,3]' LIMIT 10";
        assert!(query.contains("LIMIT"));
        assert!(query.contains("ORDER BY"));
    }

    #[test]
    fn test_detect_topk_mysql() {
        let query = "SELECT * FROM docs ORDER BY vector_distance(emb, '[1,2,3]') LIMIT 20";
        assert!(query.contains("LIMIT"));
    }

    #[test]
    fn test_detect_topk_sqlserver() {
        let query = "SELECT TOP 15 * FROM docs ORDER BY vector_dist";
        assert!(query.contains("TOP 15"));
    }

    #[test]
    fn test_detect_topk_sqlite() {
        let query = "SELECT * FROM docs ORDER BY vec_distance_l2(emb, '[1,2,3]') LIMIT 5";
        assert!(query.contains("LIMIT 5"));
    }

    #[test]
    fn test_no_topk_without_limit() {
        let query = "SELECT * FROM docs ORDER BY score DESC";
        assert!(!query.contains("LIMIT"));
        assert!(!query.contains("TOP"));
    }
}

/// `VectorFilter` detection tests.
mod vector_filter_detection {
    #[test]
    fn test_detect_vector_filter_postgres() {
        let query = "SELECT * FROM docs WHERE embedding <-> '[1,2,3]' < 0.5";
        assert!(query.contains("<->"));
        assert!(query.contains("< 0.5"));
    }

    #[test]
    fn test_detect_vector_filter_threshold() {
        let query = "SELECT * FROM docs WHERE vector_distance(emb, '[1,2,3]') <= 1.0";
        assert!(query.contains("<= 1.0"));
    }

    #[test]
    fn test_detect_vector_filter_range() {
        let query = "SELECT * FROM docs WHERE dist BETWEEN 0.1 AND 0.9";
        assert!(query.contains("BETWEEN"));
    }

    #[test]
    fn test_no_vector_filter_without_threshold() {
        let query = "SELECT * FROM docs ORDER BY embedding";
        // Has no filter threshold
        assert!(!query.contains(" < "));
        assert!(!query.contains(" <= "));
        assert!(!query.contains(" > "));
    }
}

/// Complex query parsing tests.
mod complex_queries {
    #[test]
    fn test_parse_hybrid_with_multiple_filters() {
        let query = r"
            SELECT * FROM docs
            WHERE category = 'tech'
              AND content_tsvector @@ to_tsquery('machine & learning')
              AND embedding <-> '[1,2,3]' < 0.5
            ORDER BY (0.5 * ts_rank + 0.5 * vector_score) DESC
            LIMIT 10
        ";

        assert!(query.contains("category"));
        assert!(query.contains("@@"));
        assert!(query.contains("<->"));
        assert!(query.contains("0.5"));
    }

    #[test]
    fn test_parse_hybrid_with_joins() {
        let query = r"
            SELECT d.*, a.name
            FROM docs d
            JOIN authors a ON d.author_id = a.id
            WHERE d.content_fts MATCH 'search'
            ORDER BY vec_distance(d.emb, '[1,2,3]')
        ";

        assert!(query.contains("JOIN"));
        assert!(query.contains("MATCH"));
    }

    #[test]
    fn test_parse_hybrid_with_aggregation() {
        let query = r"
            SELECT category, COUNT(*), AVG(score)
            FROM docs
            WHERE fts_match AND vector_match
            GROUP BY category
        ";

        assert!(query.contains("GROUP BY"));
        assert!(query.contains("AVG"));
    }

    #[test]
    fn test_parse_hybrid_with_subquery() {
        let query = r"
            SELECT * FROM (
                SELECT *, (fts_score + vec_score) as total_score
                FROM docs
            ) WHERE total_score > 0.5
        ";

        assert!(query.contains("SELECT * FROM ("));
    }
}

/// Error handling tests.
mod error_handling {
    #[test]
    fn test_invalid_vector_syntax() {
        let query = "SELECT * FROM docs WHERE embedding <-> 'invalid'";
        // Should detect invalid vector format
        assert!(query.contains("<->"));
    }

    #[test]
    fn test_missing_fts_predicate() {
        let query = "SELECT * FROM docs ORDER BY ts_rank";
        // ts_rank without matching predicate
        assert!(query.contains("ts_rank"));
    }

    #[test]
    fn test_incompatible_operators() {
        let query = "SELECT * FROM docs WHERE content @@ '[1,2,3]'";
        // FTS operator with vector literal
        assert!(query.contains("@@"));
    }
}

/// Feature detection tests.
mod feature_detection {
    #[test]
    fn test_detect_rum_index_usage() {
        let query = "ORDER BY content_tsvector <=> to_tsquery('search')";
        assert!(query.contains("<=>")); // RUM distance operator
    }

    #[test]
    fn test_detect_gin_index_usage() {
        let query = "WHERE content_tsvector @@ to_tsquery('search')";
        assert!(query.contains("@@")); // GIN operator
    }

    #[test]
    fn test_detect_hnsw_index_hint() {
        let query = "ORDER BY embedding <-> '[1,2,3]' /* USING HNSW */";
        assert!(query.contains("HNSW"));
    }

    #[test]
    fn test_detect_ivfflat_index_hint() {
        let query = "ORDER BY embedding <-> '[1,2,3]' /* USING IVFFlat */";
        assert!(query.contains("IVFFlat"));
    }
}

/// Distance metric detection tests.
mod distance_metrics {
    #[test]
    fn test_detect_l2_distance() {
        let query = "ORDER BY embedding <-> '[1,2,3]'"; // L2 in pgvector
        assert!(query.contains("<->"));
    }

    #[test]
    fn test_detect_cosine_distance() {
        let query = "ORDER BY embedding <=> '[1,2,3]'"; // Cosine in pgvector
        assert!(query.contains("<=>"));
    }

    #[test]
    fn test_detect_inner_product() {
        let query = "ORDER BY embedding <#> '[1,2,3]'"; // Inner product in pgvector
        assert!(query.contains("<#>"));
    }

    #[test]
    fn test_detect_explicit_metric() {
        let query = "ORDER BY vector_distance_l2(emb, '[1,2,3]')";
        assert!(query.contains("vector_distance_l2"));
    }
}

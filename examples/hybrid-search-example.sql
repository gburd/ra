-- Example: Hybrid Search Query with Vector Similarity + Full-Text Search
-- This query demonstrates Ra's hybrid search optimization capabilities

-- Create sample schema (for context - not executed by ra-cli)
-- CREATE TABLE articles (
--     id SERIAL PRIMARY KEY,
--     title TEXT,
--     body TEXT,
--     embedding vector(768),
--     body_tsv tsvector
-- );
-- CREATE INDEX idx_embedding_hnsw ON articles USING hnsw (embedding vector_l2_ops);
-- CREATE INDEX idx_body_tsv_rum ON articles USING rum (body_tsv rum_tsvector_addon_ops);

-- Hybrid search query combining vector similarity and full-text search
SELECT
    id,
    title,
    -- BM25 score from full-text search
    ts_rank(body_tsv, to_tsquery('english', 'database & optimization')) AS bm25_score,
    -- Vector similarity score (1 - distance)
    1 - (embedding <-> '[0.1, 0.2, 0.3]'::vector) AS vector_score,
    -- Hybrid score: weighted combination (alpha=0.7 for BM25, 0.3 for vector)
    (0.7 * ts_rank(body_tsv, to_tsquery('english', 'database & optimization')) +
     0.3 * (1 - (embedding <-> '[0.1, 0.2, 0.3]'::vector))) AS hybrid_score
FROM articles
WHERE
    -- Full-text pre-filter (selective)
    body_tsv @@ to_tsquery('english', 'database & optimization')
    -- Vector distance threshold
    AND embedding <-> '[0.1, 0.2, 0.3]'::vector < 0.5
ORDER BY hybrid_score DESC
LIMIT 10;

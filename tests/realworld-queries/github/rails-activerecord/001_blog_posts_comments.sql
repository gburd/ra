-- Rails Blog Application
-- Source: Typical Rails apps (Discourse, Forem, Refinery CMS)
-- Pattern: OLTP with polymorphic associations

CREATE TABLE posts (
    id BIGINT PRIMARY KEY,
    title VARCHAR(255) NOT NULL,
    body TEXT NOT NULL,
    author_id INTEGER NOT NULL,
    category_id INTEGER,
    published_at TIMESTAMP NULL,
    views_count INTEGER NOT NULL DEFAULT 0,
    comments_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE comments (
    id BIGINT PRIMARY KEY,
    commentable_type VARCHAR(255) NOT NULL,
    commentable_id BIGINT NOT NULL,
    author_id INTEGER NOT NULL,
    body TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    username VARCHAR(100) NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE INDEX idx_posts_author_id ON posts(author_id);
CREATE INDEX idx_posts_published_at ON posts(published_at);
CREATE INDEX idx_comments_commentable ON comments(commentable_type, commentable_id);
CREATE INDEX idx_comments_author_id ON comments(author_id);

-- ActiveRecord query: Posts with comment counts
SELECT
    p.id,
    p.title,
    p.published_at,
    p.views_count,
    u.username AS author_username,
    COUNT(c.id) AS comment_count
FROM posts p
LEFT JOIN users u ON p.author_id = u.id
LEFT JOIN comments c ON c.commentable_type = 'Post'
    AND c.commentable_id = p.id
WHERE p.published_at IS NOT NULL
    AND p.published_at <= CURRENT_TIMESTAMP
GROUP BY p.id, p.title, p.published_at, p.views_count, u.username
ORDER BY p.published_at DESC
LIMIT 20;

-- Popular posts (Rails counter_cache pattern)
SELECT
    p.id,
    p.title,
    p.author_id,
    p.views_count,
    p.comments_count,
    p.published_at
FROM posts p
WHERE p.published_at IS NOT NULL
    AND p.published_at >= CURRENT_TIMESTAMP - INTERVAL '7 days'
ORDER BY (p.views_count * 0.7 + p.comments_count * 0.3) DESC
LIMIT 10;

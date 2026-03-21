-- Forum/Discussion Platform Queries
-- Source: Discourse, Flarum, NodeBB
-- Pattern: OLTP/OLAP hybrid - Social interaction tracking

CREATE TABLE categories (
    id INTEGER PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL UNIQUE,
    description TEXT,
    parent_id INTEGER NULL,
    position INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE topics (
    id BIGINT PRIMARY KEY,
    category_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    title VARCHAR(500) NOT NULL,
    slug VARCHAR(500) NOT NULL,
    views_count INTEGER NOT NULL DEFAULT 0,
    posts_count INTEGER NOT NULL DEFAULT 0,
    likes_count INTEGER NOT NULL DEFAULT 0,
    is_pinned BOOLEAN NOT NULL DEFAULT FALSE,
    is_locked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    last_post_at TIMESTAMP NULL
);

CREATE TABLE posts (
    id BIGINT PRIMARY KEY,
    topic_id BIGINT NOT NULL,
    user_id INTEGER NOT NULL,
    content TEXT NOT NULL,
    likes_count INTEGER NOT NULL DEFAULT 0,
    is_first_post BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP NULL
);

CREATE TABLE likes (
    id BIGINT PRIMARY KEY,
    user_id INTEGER NOT NULL,
    likeable_type VARCHAR(50) NOT NULL,
    likeable_id BIGINT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    UNIQUE(user_id, likeable_type, likeable_id)
);

CREATE INDEX idx_topics_category_updated ON topics(category_id, updated_at DESC);
CREATE INDEX idx_topics_last_post_at ON topics(last_post_at DESC);
CREATE INDEX idx_posts_topic_created ON posts(topic_id, created_at);
CREATE INDEX idx_likes_likeable ON likes(likeable_type, likeable_id);

-- Query: Forum homepage (hot topics)
SELECT
    t.id,
    t.title,
    t.slug,
    t.views_count,
    t.posts_count,
    t.likes_count,
    t.is_pinned,
    t.created_at,
    t.last_post_at,
    c.name AS category_name,
    c.slug AS category_slug,
    -- Activity score for ranking
    (
        t.posts_count * 2.0 +
        t.likes_count * 3.0 +
        t.views_count * 0.1 +
        CASE WHEN t.last_post_at > CURRENT_TIMESTAMP - INTERVAL '24 hours' THEN 50 ELSE 0 END
    ) * EXP(-EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - t.last_post_at)) / 86400.0 / 7.0) AS activity_score
FROM topics t
JOIN categories c ON t.category_id = c.id
WHERE t.deleted_at IS NULL
    AND t.is_locked = FALSE
ORDER BY t.is_pinned DESC, activity_score DESC, t.last_post_at DESC
LIMIT 50;

-- Query: Topic detail with posts
SELECT
    p.id,
    p.user_id,
    p.content,
    p.likes_count,
    p.is_first_post,
    p.created_at,
    p.updated_at,
    COUNT(l.id) AS user_likes_count
FROM posts p
LEFT JOIN likes l ON l.likeable_type = 'Post'
    AND l.likeable_id = p.id
WHERE p.topic_id = 12345
    AND p.deleted_at IS NULL
GROUP BY p.id, p.user_id, p.content, p.likes_count, p.is_first_post, p.created_at, p.updated_at
ORDER BY p.is_first_post DESC, p.created_at ASC;

-- Query: User engagement metrics
WITH user_activity AS (
    SELECT
        user_id,
        COUNT(DISTINCT t.id) AS topics_created,
        COUNT(DISTINCT p.id) AS posts_created,
        SUM(p.likes_count) AS total_likes_received,
        MIN(COALESCE(t.created_at, p.created_at)) AS first_activity,
        MAX(COALESCE(t.created_at, p.created_at)) AS last_activity
    FROM users u
    LEFT JOIN topics t ON u.id = t.user_id
        AND t.created_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
    LEFT JOIN posts p ON u.id = p.user_id
        AND p.created_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
        AND p.deleted_at IS NULL
    GROUP BY user_id
),
user_given_likes AS (
    SELECT
        user_id,
        COUNT(*) AS likes_given
    FROM likes
    WHERE created_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
    GROUP BY user_id
)
SELECT
    ua.user_id,
    ua.topics_created,
    ua.posts_created,
    ua.total_likes_received,
    COALESCE(ugl.likes_given, 0) AS likes_given,
    EXTRACT(EPOCH FROM (ua.last_activity - ua.first_activity)) / 86400 AS active_days,
    -- Engagement score
    (
        ua.topics_created * 5.0 +
        ua.posts_created * 2.0 +
        ua.total_likes_received * 3.0 +
        COALESCE(ugl.likes_given, 0) * 1.0
    ) AS engagement_score
FROM user_activity ua
LEFT JOIN user_given_likes ugl ON ua.user_id = ugl.user_id
WHERE ua.topics_created > 0 OR ua.posts_created > 0
ORDER BY engagement_score DESC
LIMIT 100;

-- Query: Category statistics
SELECT
    c.id,
    c.name,
    COUNT(DISTINCT t.id) AS topic_count,
    COUNT(DISTINCT p.id) AS post_count,
    COUNT(DISTINCT t.user_id) AS unique_authors,
    MAX(t.last_post_at) AS last_activity,
    SUM(t.views_count) AS total_views
FROM categories c
LEFT JOIN topics t ON c.id = t.category_id
LEFT JOIN posts p ON t.id = p.topic_id AND p.deleted_at IS NULL
WHERE c.parent_id IS NULL  -- Top-level categories only
GROUP BY c.id, c.name
ORDER BY total_views DESC;

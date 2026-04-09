-- Blog Platform Schema Data Generation
-- Generates 1M users, 10M posts, 50M comments, 1000 tags, 20M post_tags

\echo 'Generating Blog Platform test data...'

-- Generate 1,000,000 users
INSERT INTO users (username, email, password_hash, display_name, bio, created_at, last_login)
SELECT
  'user' || i,
  'user' || i || '@example.com',
  '$2b$10$' || md5(random()::text),
  'User ' || i,
  'Bio for user ' || i,
  CURRENT_TIMESTAMP - (random() * 1825 || ' days')::interval,
  CURRENT_TIMESTAMP - (random() * 30 || ' days')::interval
FROM generate_series(1, 1000000) AS i;

\echo 'Generated 1,000,000 users'

-- Generate 10,000,000 posts (in batches)
DO $$
DECLARE
  batch_size INT := 100000;
  total_posts INT := 10000000;
  batch_count INT;
BEGIN
  batch_count := total_posts / batch_size;

  FOR batch IN 1..batch_count LOOP
    INSERT INTO posts (
      author_id, title, slug, content, excerpt,
      status, published_at, created_at, updated_at
    )
    SELECT
      1 + (random() * 999999)::int,
      'Post Title ' || ((batch - 1) * batch_size + i),
      'post-slug-' || ((batch - 1) * batch_size + i),
      'Content for post ' || ((batch - 1) * batch_size + i) || '. ' || repeat('Lorem ipsum dolor sit amet. ', 20),
      'Excerpt for post ' || ((batch - 1) * batch_size + i),
      (ARRAY['draft', 'published', 'archived'])[1 + (random() * 2)::int],
      CASE WHEN random() > 0.3 THEN CURRENT_TIMESTAMP - (random() * 730 || ' days')::interval ELSE NULL END,
      CURRENT_TIMESTAMP - (random() * 730 || ' days')::interval,
      CURRENT_TIMESTAMP - (random() * 365 || ' days')::interval
    FROM generate_series(1, batch_size) AS i;

    RAISE NOTICE 'Generated batch % of % (%% posts)', batch, batch_count, (batch * 100.0 / batch_count)::int;
  END LOOP;
END $$;

\echo 'Generated 10,000,000 posts'

-- Generate 1,000 tags
INSERT INTO tags (name, slug)
SELECT
  'Tag ' || i,
  'tag-' || i
FROM generate_series(1, 1000) AS i;

\echo 'Generated 1,000 tags'

-- Generate 20,000,000 post_tags
DO $$
DECLARE
  batch_size INT := 500000;
  total_tags INT := 20000000;
  batch_count INT;
BEGIN
  batch_count := total_tags / batch_size;

  FOR batch IN 1..batch_count LOOP
    INSERT INTO post_tags (post_id, tag_id)
    SELECT DISTINCT ON (post_id, tag_id)
      1 + (random() * 9999999)::int,
      1 + (random() * 999)::int
    FROM generate_series(1, batch_size) AS i
    ON CONFLICT DO NOTHING;

    RAISE NOTICE 'Generated batch % of % post_tags (%% complete)', batch, batch_count, (batch * 100.0 / batch_count)::int;
  END LOOP;
END $$;

\echo 'Generated 20,000,000 post_tags'

-- Generate 50,000,000 comments (in batches)
DO $$
DECLARE
  batch_size INT := 500000;
  total_comments INT := 50000000;
  batch_count INT;
BEGIN
  batch_count := total_comments / batch_size;

  FOR batch IN 1..batch_count LOOP
    INSERT INTO comments (
      post_id, author_id, parent_id, content, status, created_at
    )
    SELECT
      1 + (random() * 9999999)::int,
      CASE WHEN random() > 0.1 THEN 1 + (random() * 999999)::int ELSE NULL END,
      CASE WHEN random() > 0.8 THEN 1 + (random() * (batch * batch_size - 1))::int ELSE NULL END,
      'Comment ' || ((batch - 1) * batch_size + i) || ': ' || repeat('Some comment text. ', 5),
      (ARRAY['pending', 'approved', 'spam'])[1 + (random() * 2)::int],
      CURRENT_TIMESTAMP - (random() * 730 || ' days')::interval
    FROM generate_series(1, batch_size) AS i;

    RAISE NOTICE 'Generated batch % of % comments (%% complete)', batch, batch_count, (batch * 100.0 / batch_count)::int;
  END LOOP;
END $$;

\echo 'Generated 50,000,000 comments'

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_posts_author_id ON posts(author_id);
CREATE INDEX IF NOT EXISTS idx_posts_status ON posts(status);
CREATE INDEX IF NOT EXISTS idx_posts_published_at ON posts(published_at);
CREATE INDEX IF NOT EXISTS idx_posts_slug ON posts(slug);
CREATE INDEX IF NOT EXISTS idx_comments_post_id ON comments(post_id);
CREATE INDEX IF NOT EXISTS idx_comments_author_id ON comments(author_id);
CREATE INDEX IF NOT EXISTS idx_comments_parent_id ON comments(parent_id);
CREATE INDEX IF NOT EXISTS idx_comments_status ON comments(status);
CREATE INDEX IF NOT EXISTS idx_post_tags_post_id ON post_tags(post_id);
CREATE INDEX IF NOT EXISTS idx_post_tags_tag_id ON post_tags(tag_id);
CREATE INDEX IF NOT EXISTS idx_tags_slug ON tags(slug);

\echo 'Blog Platform test data generation complete'

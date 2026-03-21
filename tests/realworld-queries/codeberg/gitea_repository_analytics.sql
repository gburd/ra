-- Gitea/Codeberg Repository Analytics
-- Source: Git forge platforms (Gitea, Forgejo, Gogs)
-- Pattern: OLAP - Code repository metrics

CREATE TABLE repositories (
    id BIGINT PRIMARY KEY,
    owner_id INTEGER NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    is_private BOOLEAN NOT NULL DEFAULT FALSE,
    fork_id BIGINT NULL,
    size_kb BIGINT NOT NULL DEFAULT 0,
    default_branch VARCHAR(100) NOT NULL DEFAULT 'main',
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE commits (
    id BIGINT PRIMARY KEY,
    repo_id BIGINT NOT NULL,
    sha VARCHAR(40) NOT NULL,
    author_id INTEGER NOT NULL,
    message TEXT NOT NULL,
    additions INTEGER NOT NULL DEFAULT 0,
    deletions INTEGER NOT NULL DEFAULT 0,
    files_changed INTEGER NOT NULL DEFAULT 0,
    committed_at TIMESTAMP NOT NULL
);

CREATE TABLE pull_requests (
    id BIGINT PRIMARY KEY,
    repo_id BIGINT NOT NULL,
    title VARCHAR(500) NOT NULL,
    author_id INTEGER NOT NULL,
    status VARCHAR(20) NOT NULL,
    head_branch VARCHAR(255) NOT NULL,
    base_branch VARCHAR(255) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    merged_at TIMESTAMP NULL,
    closed_at TIMESTAMP NULL
);

CREATE TABLE issues (
    id BIGINT PRIMARY KEY,
    repo_id BIGINT NOT NULL,
    title VARCHAR(500) NOT NULL,
    author_id INTEGER NOT NULL,
    status VARCHAR(20) NOT NULL,
    labels TEXT[],
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    closed_at TIMESTAMP NULL
);

CREATE INDEX idx_commits_repo_time ON commits(repo_id, committed_at DESC);
CREATE INDEX idx_pull_requests_repo_status ON pull_requests(repo_id, status);
CREATE INDEX idx_issues_repo_status ON issues(repo_id, status);

-- Query: Repository activity summary
SELECT
    r.id,
    r.name,
    r.description,
    COUNT(DISTINCT c.id) AS commit_count_30d,
    COUNT(DISTINCT pr.id) AS pr_count_30d,
    COUNT(DISTINCT i.id) AS issue_count_30d,
    COUNT(DISTINCT c.author_id) AS active_contributors_30d,
    SUM(c.additions + c.deletions) AS total_changes_30d
FROM repositories r
LEFT JOIN commits c ON r.id = c.repo_id
    AND c.committed_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
LEFT JOIN pull_requests pr ON r.id = pr.repo_id
    AND pr.created_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
LEFT JOIN issues i ON r.id = i.repo_id
    AND i.created_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
WHERE r.is_private = FALSE
GROUP BY r.id, r.name, r.description
HAVING COUNT(DISTINCT c.id) > 0
ORDER BY total_changes_30d DESC
LIMIT 50;

-- Query: Contributor activity ranking
SELECT
    c.author_id,
    COUNT(DISTINCT c.repo_id) AS repos_contributed,
    COUNT(*) AS total_commits,
    SUM(c.additions) AS total_additions,
    SUM(c.deletions) AS total_deletions,
    SUM(c.files_changed) AS total_files_changed,
    MIN(c.committed_at) AS first_commit,
    MAX(c.committed_at) AS last_commit
FROM commits c
WHERE c.committed_at >= CURRENT_TIMESTAMP - INTERVAL '90 days'
GROUP BY c.author_id
HAVING COUNT(*) >= 10
ORDER BY total_commits DESC
LIMIT 100;

-- Query: Pull request metrics
WITH pr_metrics AS (
    SELECT
        pr.id,
        pr.repo_id,
        pr.title,
        pr.author_id,
        pr.created_at,
        pr.merged_at,
        pr.closed_at,
        EXTRACT(EPOCH FROM (
            COALESCE(pr.merged_at, pr.closed_at, CURRENT_TIMESTAMP) - pr.created_at
        )) / 3600 AS pr_lifetime_hours,
        CASE
            WHEN pr.merged_at IS NOT NULL THEN 'merged'
            WHEN pr.closed_at IS NOT NULL THEN 'closed'
            ELSE 'open'
        END AS pr_outcome
    FROM pull_requests pr
    WHERE pr.created_at >= CURRENT_TIMESTAMP - INTERVAL '90 days'
)
SELECT
    repo_id,
    COUNT(*) AS total_prs,
    SUM(CASE WHEN pr_outcome = 'merged' THEN 1 ELSE 0 END) AS merged_count,
    SUM(CASE WHEN pr_outcome = 'closed' THEN 1 ELSE 0 END) AS closed_count,
    SUM(CASE WHEN pr_outcome = 'open' THEN 1 ELSE 0 END) AS open_count,
    AVG(pr_lifetime_hours) FILTER (WHERE pr_outcome = 'merged') AS avg_merge_time_hours,
    PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY pr_lifetime_hours)
        FILTER (WHERE pr_outcome = 'merged') AS median_merge_time_hours
FROM pr_metrics
GROUP BY repo_id
HAVING COUNT(*) >= 5
ORDER BY total_prs DESC;

-- Query: Issue triage dashboard
SELECT
    i.repo_id,
    i.status,
    COUNT(*) AS issue_count,
    ARRAY_AGG(DISTINCT label ORDER BY label) FILTER (WHERE label IS NOT NULL) AS all_labels,
    AVG(EXTRACT(EPOCH FROM (
        COALESCE(i.closed_at, CURRENT_TIMESTAMP) - i.created_at
    )) / 86400) AS avg_age_days,
    MAX(i.created_at) AS newest_issue,
    MIN(i.created_at) AS oldest_issue
FROM issues i
CROSS JOIN LATERAL UNNEST(
    CASE WHEN i.labels IS NOT NULL THEN i.labels ELSE ARRAY[]::TEXT[] END
) AS label
WHERE i.created_at >= CURRENT_TIMESTAMP - INTERVAL '180 days'
GROUP BY i.repo_id, i.status
ORDER BY i.repo_id, i.status;

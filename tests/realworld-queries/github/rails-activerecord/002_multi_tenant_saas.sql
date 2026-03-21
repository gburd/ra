-- Multi-Tenant SaaS Application
-- Source: Rails SaaS apps (Shopify, Basecamp patterns)
-- Pattern: OLTP with tenant isolation

CREATE TABLE tenants (
    id INTEGER PRIMARY KEY,
    subdomain VARCHAR(100) NOT NULL UNIQUE,
    plan VARCHAR(50) NOT NULL,
    status VARCHAR(20) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE projects (
    id BIGINT PRIMARY KEY,
    tenant_id INTEGER NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    owner_id INTEGER NOT NULL,
    status VARCHAR(50) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    archived_at TIMESTAMP NULL
);

CREATE TABLE tasks (
    id BIGINT PRIMARY KEY,
    tenant_id INTEGER NOT NULL,
    project_id BIGINT NOT NULL,
    title VARCHAR(500) NOT NULL,
    description TEXT,
    assignee_id INTEGER,
    status VARCHAR(50) NOT NULL,
    priority INTEGER NOT NULL DEFAULT 3,
    due_date DATE,
    completed_at TIMESTAMP NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

CREATE TABLE team_members (
    id BIGINT PRIMARY KEY,
    tenant_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    role VARCHAR(50) NOT NULL,
    created_at TIMESTAMP NOT NULL
);

-- Tenant isolation indexes
CREATE INDEX idx_projects_tenant_id ON projects(tenant_id);
CREATE INDEX idx_tasks_tenant_id ON tasks(tenant_id);
CREATE INDEX idx_tasks_project_id ON tasks(project_id);
CREATE INDEX idx_tasks_assignee_id ON tasks(assignee_id);
CREATE INDEX idx_team_members_tenant_user ON team_members(tenant_id, user_id);

-- Query: Tenant-isolated project dashboard
SELECT
    p.id,
    p.name,
    p.status,
    COUNT(t.id) AS total_tasks,
    SUM(CASE WHEN t.status = 'completed' THEN 1 ELSE 0 END) AS completed_tasks,
    SUM(CASE WHEN t.status = 'in_progress' THEN 1 ELSE 0 END) AS in_progress_tasks,
    SUM(CASE WHEN t.due_date < CURRENT_DATE AND t.status != 'completed'
        THEN 1 ELSE 0 END) AS overdue_tasks
FROM projects p
LEFT JOIN tasks t ON p.id = t.project_id AND t.tenant_id = p.tenant_id
WHERE p.tenant_id = 123  -- Always filtered by tenant
    AND p.archived_at IS NULL
GROUP BY p.id, p.name, p.status
ORDER BY p.updated_at DESC;

-- Query: User workload (tenant-scoped)
SELECT
    tm.user_id,
    COUNT(t.id) AS assigned_tasks,
    SUM(CASE WHEN t.status = 'in_progress' THEN 1 ELSE 0 END) AS active_tasks,
    SUM(CASE WHEN t.due_date < CURRENT_DATE AND t.status != 'completed'
        THEN 1 ELSE 0 END) AS overdue_tasks,
    SUM(CASE WHEN t.priority = 1 THEN 1 ELSE 0 END) AS high_priority_tasks
FROM team_members tm
LEFT JOIN tasks t ON tm.user_id = t.assignee_id
    AND tm.tenant_id = t.tenant_id
WHERE tm.tenant_id = 123
    AND tm.role IN ('member', 'admin')
GROUP BY tm.user_id
ORDER BY active_tasks DESC;

-- Query: Cross-project task dependencies (tenant-scoped)
WITH project_stats AS (
    SELECT
        project_id,
        tenant_id,
        COUNT(*) AS task_count,
        AVG(CASE WHEN status = 'completed' THEN 1.0 ELSE 0.0 END) AS completion_rate,
        AVG(EXTRACT(EPOCH FROM (completed_at - created_at)) / 86400.0)
            FILTER (WHERE completed_at IS NOT NULL) AS avg_completion_days
    FROM tasks
    WHERE tenant_id = 123
    GROUP BY project_id, tenant_id
)
SELECT
    p.id,
    p.name,
    ps.task_count,
    ROUND(ps.completion_rate * 100, 2) AS completion_pct,
    ROUND(ps.avg_completion_days, 1) AS avg_completion_days
FROM projects p
JOIN project_stats ps ON p.id = ps.project_id
WHERE p.tenant_id = 123
    AND p.archived_at IS NULL
ORDER BY ps.completion_rate DESC;

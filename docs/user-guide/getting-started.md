# Getting Started

This guide walks you through installing and using the RA Web query optimization interface.

## Quick Start with Docker Compose

The easiest way to get started is using Docker Compose, which sets up all required services automatically.

### Prerequisites

- Docker (version 20.10+)
- Docker Compose (version 2.0+)
- 4GB+ available RAM
- 10GB+ available disk space

### Installation

1. Clone the repository:

```bash
git clone https://github.com/yourusername/ra.git
cd ra
```

2. Start all services:

```bash
docker-compose up -d
```

This launches the following services:

- **ra-web** (port 8000) - Query optimization web interface
- **postgres-15** (port 5415) - PostgreSQL 15 test database
- **postgres-16** (port 5416) - PostgreSQL 16 test database
- **mysql-8** (port 3306) - MySQL 8.0 test database
- **mariadb** (port 3307) - MariaDB 11 test database
- **duckdb** (port 8080) - DuckDB service
- **redis** (port 6379) - Results cache
- **docs** (port 3000) - Documentation site

3. Wait for services to become healthy:

```bash
docker-compose ps
```

All services should show status "healthy" (this takes 30-60 seconds on first startup).

4. Open the web interface:

```
http://localhost:8000
```

### Test Database Schemas

The Docker Compose setup automatically loads five test schemas:

- **HR** - Employee and department management
- **E-Commerce** - Customers, orders, and products
- **TPC-H** - Industry standard benchmark schema
- **Sakila** - DVD rental store (film, actor, rental)
- **Blog** - Blog platform with posts, comments, and tags

Each schema includes sample data for testing queries.

## First Query Execution

Let's run your first query optimization:

1. The editor opens with a default query:

```sql
SELECT * FROM employees
WHERE department_id = 1;
```

2. Select an engine from the dropdown (default: PostgreSQL 16)

3. Choose EXPLAIN mode:
   - **explain** - Show query plan without execution
   - **analyze** - Execute query and show actual performance

4. Click **Execute** (or press Ctrl+Enter)

5. View the results in the output panel

### Understanding the Output

The output panel shows the database's query execution plan with five visualization tabs:

**Raw Plan** - Text view with syntax highlighting, collapsible nodes, and search

**Tree View** - Hierarchical visualization showing parent-child relationships between operations

**Flow View** - Data flow diagram showing how data moves through operations

**Cost Analysis** - Bar charts and tables comparing estimated vs. actual costs

**Warnings** - Detected optimization opportunities and performance issues

See [Visualizations](./visualizations.md) for detailed explanations.

## Navigation Basics

### Editor Pane (Left)

- Write or paste SQL queries
- Syntax highlighting for SQL
- Ctrl+Enter to execute
- Auto-save to URL on execution

### Output Panel (Right)

- Engine selector dropdown
- Visualization tabs
- Copy button (top-right)
- Search button (magnifying glass icon)

### Toolbar (Top)

- **Execute** - Run query on all panels
- **Mode Toggle** - Switch between explain/analyze
- **Add Panel** - Compare up to 4 engines side-by-side
- **Schema** - View table definitions and sample queries
- **Share** - Generate shareable URL

## Multiple Engine Comparison

Compare query plans across different database engines:

1. Click **Add Panel** (up to 4 panels total)

2. Select different engines in each panel dropdown

3. Click **Execute** to run the same query on all engines

4. Compare the results side-by-side

Example comparison scenarios:

- PostgreSQL 15 vs. PostgreSQL 16 (version differences)
- PostgreSQL vs. MySQL (dialect differences)
- DuckDB vs. PostgreSQL (analytical vs. OLTP)

See [Comparison Features](./comparison-features.md) for advanced comparison techniques.

## Using Sample Schemas

The **Schema** button opens a dialog with:

1. Five schema tabs (HR, E-Commerce, TPC-H, Sakila, Blog)

2. Two content tabs:
   - **Tables** - DDL definitions for all tables
   - **Sample Queries** - Pre-written queries to load

3. Click any sample query to load it into the editor

This helps you quickly test queries without writing DDL.

## Keyboard Shortcuts

- **Ctrl+Enter** - Execute query
- **Ctrl+F** - Open search (in output panel)
- **F3 / Shift+F3** - Next/previous search match
- **Escape** - Close search bar or dialogs

## Sharing Queries

Generate a shareable URL:

1. Click the **Share** button

2. Copy the generated URL

3. Send to colleagues or bookmark for later

The URL encodes:
- SQL query text
- Selected engines
- EXPLAIN/ANALYZE mode

Recipients can click the URL to see your exact setup.

## Troubleshooting

### Services won't start

Check Docker logs:

```bash
docker-compose logs ra-web
docker-compose logs postgres-16
```

Common issues:
- Port conflicts (another service using 5432, 8000, etc.)
- Insufficient memory (increase Docker memory limit)
- Old containers (run `docker-compose down -v` to reset)

### Query execution times out

- Check the database is healthy: `docker-compose ps`
- Increase timeout in code (default: 30 seconds)
- Simplify the query or add LIMIT clause

### Missing tables or data

Reset test databases:

```bash
docker-compose down -v  # Remove volumes
docker-compose up -d    # Recreate with fresh data
```

### Page doesn't load

- Verify ra-web is running: `docker-compose ps ra-web`
- Check port 8000 is accessible: `curl http://localhost:8000/health`
- Clear browser cache and reload

## Next Steps

- [Visualizations Guide](./visualizations.md) - Learn to interpret each visualization mode
- [Comparison Features](./comparison-features.md) - Master multi-engine comparisons
- [Sample Schemas](./sample-schemas.md) - Understand the test data
- [API Reference](../reference/api-reference.md) - Use the REST API directly

## Configuration

### Environment Variables

Customize ra-web behavior in `docker-compose.yml`:

```yaml
environment:
  - RUST_LOG=info              # Logging level (debug, info, warn, error)
  - ROCKET_PORT=8000           # HTTP server port
  - DATABASE_URL=postgresql:// # Primary database connection
  - REDIS_URL=redis://         # Cache connection
```

### Adding Custom Schemas

Mount your SQL files into the containers:

```yaml
volumes:
  - ./my-schemas:/docker-entrypoint-initdb.d:ro
```

SQL files run automatically on container startup.

### Production Deployment

For production use:

1. Change default passwords in `docker-compose.yml`
2. Use external PostgreSQL (not Docker container)
3. Enable TLS for Redis
4. Set `RUST_LOG=warn` for less verbose logging
5. Configure reverse proxy (nginx/Caddy) for HTTPS

See [Deployment Guide](../deployment.md) for Fly.io, Railway, and self-hosted options.

# Docker Deployment Infrastructure

Comprehensive Docker setup for the Ra query optimizer project.

## Services

### Core Services

#### docs
- **Purpose**: Documentation site built with VitePress
- **Port**: 3000
- **Technology**: Node.js, VitePress, Nginx
- **Access**: http://localhost:3000

#### ra-web
- **Purpose**: Web API server for query optimization
- **Port**: 8000
- **Technology**: Rust, Rocket.rs
- **Access**: http://localhost:8000
- **Endpoints**:
  - `/health` - Health check
  - `/api/optimize` - Optimize queries
  - `/api/translate` - Translate SQL dialects
  - `/api/explain` - Explain query plans
  - `/api/visualize` - Visualize query plans

#### redis
- **Purpose**: Caching layer for query plans and statistics
- **Port**: 6379
- **Technology**: Redis 7
- **Persistence**: Enabled with AOF

### PostgreSQL Services

#### postgres-ra-extension
- **Purpose**: PostgreSQL 16 with Ra planner extension installed
- **Port**: 5432
- **Technology**: PostgreSQL 16, pgrx
- **Features**:
  - Native Ra optimizer integration
  - Transparent query optimization
  - Plan caching
  - Statistics collection

#### postgres-ra-proxy
- **Purpose**: PostgreSQL 19 (from source) with Ra proxy and pg_plan_advice
- **Ports**:
  - 5433 (PostgreSQL)
  - 8001 (Ra proxy API)
- **Technology**: PostgreSQL 19 (git main), Rust proxy
- **Features**:
  - Query interception and logging
  - Plan comparison (PostgreSQL vs Ra)
  - Optional plan injection via pg_plan_advice
  - Performance metrics

### Test Databases

#### postgres-15
- **Port**: 5415
- **Purpose**: PostgreSQL 15 compatibility testing

#### postgres-16
- **Port**: 5416
- **Purpose**: PostgreSQL 16 compatibility testing

#### mysql-8
- **Port**: 3306
- **Purpose**: MySQL 8.0 compatibility testing

#### mariadb
- **Port**: 3307
- **Purpose**: MariaDB 11 compatibility testing

#### duckdb
- **Port**: 8080
- **Purpose**: DuckDB compatibility testing

## Quick Start

### Start all services
```bash
docker compose up -d
```

### Start specific services
```bash
# Start only core services
docker compose up -d docs ra-web redis postgres-ra-extension

# Start only test databases
docker compose up -d postgres-15 postgres-16 mysql-8 mariadb
```

### View logs
```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f ra-web
docker compose logs -f postgres-ra-proxy
```

### Stop services
```bash
docker compose down

# Stop and remove volumes
docker compose down -v
```

## Build Details

### Multi-Stage Builds

All Dockerfiles use multi-stage builds for smaller production images:

1. **docs**: Node.js builder → Nginx runtime (< 50MB)
2. **ra-web**: cargo-chef caching → Alpine runtime (< 100MB)
3. **postgres-ra-extension**: Rust builder → PostgreSQL runtime
4. **postgres-ra-proxy**: PostgreSQL source build + Rust proxy → Debian runtime

### Build Arguments

Build specific images:
```bash
# Build docs
docker compose build docs

# Build ra-web
docker compose build ra-web

# Build PostgreSQL with Ra extension
docker compose build postgres-ra-extension

# Build PostgreSQL 19 with proxy
docker compose build postgres-ra-proxy
```

### Rebuild from scratch
```bash
docker compose build --no-cache
```

## Testing

### Test ra-web API
```bash
# Health check
curl http://localhost:8000/health

# Optimize a query
curl -X POST http://localhost:8000/api/optimize \
  -H "Content-Type: application/json" \
  -d '{"expr":{"Scan":{"table":"users"}}}'

# Translate SQL
curl -X POST http://localhost:8000/api/translate \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT * FROM users","from":"pg","to":"mysql"}'
```

### Test PostgreSQL with Ra extension
```bash
# Connect to PostgreSQL
psql -h localhost -p 5432 -U ra_test -d ra_testdb

# Check if Ra extension is loaded
\dx pg_ra_planner

# Run a query with Ra optimization
EXPLAIN (ANALYZE, COSTS, VERBOSE)
SELECT * FROM users WHERE age > 25;
```

### Test PostgreSQL 19 proxy
```bash
# Connect to PostgreSQL 19
psql -h localhost -p 5433 -U ra_proxy -d ra_proxydb

# Run a query (proxy will intercept and compare plans)
SELECT * FROM test_table WHERE id > 100;

# Check proxy logs
docker compose logs postgres-ra-proxy

# Query proxy API for plan comparisons
curl http://localhost:8001/health
```

## Configuration

### Environment Variables

Edit `docker-compose.yml` to configure services:

#### ra-web
- `RUST_LOG`: Log level (debug, info, warn, error)
- `ROCKET_PORT`: Server port
- `DATABASE_URL`: PostgreSQL connection string
- `REDIS_URL`: Redis connection string

#### postgres-ra-proxy
- `RA_PROXY_PORT`: Proxy API port
- `RA_PROXY_LOG_LEVEL`: Logging level
- `RA_PROXY_COMPARE_PLANS`: Enable plan comparison
- `RA_PROXY_INJECT_PLANS`: Enable plan injection via pg_plan_advice

### Volumes

Persistent storage locations:
- `pg-ra-extension-data`: PostgreSQL 16 data
- `pg-ra-proxy-data`: PostgreSQL 19 data
- `redis-data`: Redis persistence
- `pg15-data`, `pg16-data`: Test database data
- `mysql8-data`, `mariadb-data`: MySQL data

### Networks

All services communicate via the `ra-network` bridge network.

## Troubleshooting

### Check service health
```bash
docker compose ps
```

### View service logs
```bash
docker compose logs -f <service-name>
```

### Restart a service
```bash
docker compose restart <service-name>
```

### Reset everything
```bash
docker compose down -v
docker compose up -d
```

### Check disk usage
```bash
docker system df
docker volume ls
```

### Clean up unused resources
```bash
docker system prune -a --volumes
```

## Production Deployment

### Security Checklist

1. Change default passwords in `docker-compose.yml`
2. Use secrets for sensitive data
3. Enable TLS for all services
4. Configure firewall rules
5. Set up log aggregation
6. Enable monitoring and alerts

### Performance Tuning

1. Adjust PostgreSQL settings in `postgresql.conf`
2. Configure Redis memory limits
3. Set appropriate resource limits in `docker-compose.yml`
4. Use volume drivers optimized for your storage

### Monitoring

Services expose the following health endpoints:
- docs: `http://localhost:3000/health`
- ra-web: `http://localhost:8000/health`
- postgres-ra-proxy: `http://localhost:8001/health`

All PostgreSQL instances support `pg_isready` for health checks.

## Development

### Local Development vs Docker

For active development, run services outside Docker:
```bash
# Start only databases
docker compose up -d redis postgres-ra-extension postgres-15

# Run ra-web locally
cd crates/ra-web
cargo run

# Run docs locally
cd docs
npm run dev
```

### Debugging

Enable debug logging:
```bash
# Edit docker-compose.yml
environment:
  - RUST_LOG=debug
  - RA_PROXY_LOG_LEVEL=debug
```

Access container shell:
```bash
docker compose exec ra-web sh
docker compose exec postgres-ra-extension bash
```

## References

- [Docker Compose Documentation](https://docs.docker.com/compose/)
- [PostgreSQL Docker Image](https://hub.docker.com/_/postgres)
- [Rust Docker Best Practices](https://docs.docker.com/language/rust/)
- [cargo-chef for Docker builds](https://github.com/LukeMathWalker/cargo-chef)

# Docker Deployment Guide

Comprehensive Docker deployment infrastructure for the Ra query optimizer project.

## Overview

This deployment includes:
- **docs**: VitePress documentation site (port 3000)
- **ra-web**: Rust web API server (port 8000)
- **postgres-ra-extension**: PostgreSQL 16 with Ra planner extension (port 5432)
- **postgres-ra-proxy**: PostgreSQL 19 with Ra proxy and pg_plan_advice (ports 5433, 8001)
- **redis**: Caching layer (port 6379)
- **Test databases**: PostgreSQL 15/16, MySQL 8, MariaDB, DuckDB

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         Ra Network                           │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌─────────┐  ┌─────────┐  ┌───────────────────────┐       │
│  │  docs   │  │ ra-web  │  │ postgres-ra-extension │       │
│  │ :3000   │  │ :8000   │  │      :5432            │       │
│  └─────────┘  └─────────┘  └───────────────────────┘       │
│                     │                                        │
│                     └────┬─────────┬──────────┐             │
│                          │         │          │             │
│                  ┌───────▼──┐  ┌───▼────┐ ┌──▼──────────┐  │
│                  │  redis   │  │ PG-15  │ │ postgres-   │  │
│                  │  :6379   │  │ :5415  │ │ ra-proxy    │  │
│                  └──────────┘  ├────────┤ │ :5433,:8001 │  │
│                                │ PG-16  │ └─────────────┘  │
│                                │ :5416  │                   │
│                                ├────────┤                   │
│                                │ MySQL  │                   │
│                                │ :3306  │                   │
│                                ├────────┤                   │
│                                │MariaDB │                   │
│                                │ :3307  │                   │
│                                └────────┘                   │
└─────────────────────────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- Docker 24.0+
- Docker Compose 2.20+
- 8GB RAM minimum
- 20GB disk space

### Installation

```bash
# Clone repository
git clone https://github.com/gregburd/ra.git
cd ra

# Make scripts executable
chmod +x scripts/docker-*.sh docker/start-ra-proxy.sh

# Build all images
./scripts/docker-build.sh all

# Start all services
./scripts/docker-up.sh all

# Test services
./scripts/docker-test.sh
```

## Service Details

### Documentation Site (docs)

**Multi-stage build:**
1. Node.js 22 Alpine - Build VitePress site
2. Nginx 1.27 Alpine - Serve static files

**Features:**
- Gzip compression
- Static asset caching
- SPA routing support
- Security headers

**Access:**
```bash
# Open in browser
open http://localhost:3000

# Health check
curl http://localhost:3000/health
```

### Ra Web API (ra-web)

**Multi-stage build:**
1. cargo-chef - Cache dependencies
2. Rust 1.88 Alpine - Build binary
3. Alpine runtime - Minimal production image

**Features:**
- REST API for query optimization
- WebSocket support
- Rate limiting
- CORS enabled

**Access:**
```bash
# Health check
curl http://localhost:8000/health

# Optimize query
curl -X POST http://localhost:8000/api/optimize \
  -H "Content-Type: application/json" \
  -d '{"expr":{"Scan":{"table":"users"}}}'

# Translate SQL
curl -X POST http://localhost:8000/api/translate \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT * FROM t","from":"pg","to":"mysql"}'
```

### PostgreSQL with Ra Extension

**Multi-stage build:**
1. Rust + PostgreSQL dev - Build pgrx extension
2. PostgreSQL 16 - Install extension

**Features:**
- Native Ra optimizer integration
- Transparent query optimization
- Plan caching
- Automatic rule application

**Access:**
```bash
# Connect
psql -h localhost -p 5432 -U ra_test -d ra_testdb

# Check extension
\dx pg_ra_planner

# Run optimized query
EXPLAIN (ANALYZE, COSTS, VERBOSE)
SELECT * FROM users WHERE age > 25;
```

### PostgreSQL 19 with Ra Proxy

**Multi-stage build:**
1. Build PostgreSQL 19 from git main
2. Build pg_plan_advice extension
3. Build Ra proxy (Rust)
4. Combine in Debian runtime

**Features:**
- Query interception and logging
- Plan comparison (PostgreSQL vs Ra)
- Optional plan injection via pg_plan_advice
- Performance metrics API

**Access:**
```bash
# Connect to PostgreSQL
psql -h localhost -p 5433 -U ra_proxy -d ra_proxydb

# Query proxy API
curl http://localhost:8001/health

# View proxy logs
docker compose logs -f postgres-ra-proxy
```

**How it works:**
1. Client connects to PostgreSQL 19 on port 5433
2. Ra proxy intercepts queries before PostgreSQL
3. Proxy generates Ra-optimized plan
4. Proxy compares PostgreSQL plan vs Ra plan
5. If `RA_PROXY_INJECT_PLANS=true`, injects Ra plan via pg_plan_advice
6. Logs comparison metrics to API (port 8001)

## Build Commands

```bash
# Build all images
./scripts/docker-build.sh all

# Build specific service
./scripts/docker-build.sh docs
./scripts/docker-build.sh ra-web
./scripts/docker-build.sh postgres-ra-extension
./scripts/docker-build.sh postgres-ra-proxy

# Build core services
./scripts/docker-build.sh core

# Build PostgreSQL services
./scripts/docker-build.sh postgres

# Force rebuild without cache
./scripts/docker-build.sh all --no-cache
```

## Start/Stop Commands

```bash
# Start all services
./scripts/docker-up.sh all

# Start core services only
./scripts/docker-up.sh core

# Start test databases only
./scripts/docker-up.sh databases

# Start specific service
./scripts/docker-up.sh docs
./scripts/docker-up.sh web
./scripts/docker-up.sh postgres

# Stop all services
docker compose down

# Stop and remove volumes
docker compose down -v
```

## Testing

```bash
# Run all tests
./scripts/docker-test.sh

# Manual tests
# Test docs
curl -f http://localhost:3000/health

# Test ra-web
curl -f http://localhost:8000/health

# Test PostgreSQL extension
PGPASSWORD=ra_test_pass psql -h localhost -p 5432 -U ra_test -d ra_testdb -c 'SELECT 1;'

# Test PostgreSQL proxy
PGPASSWORD=ra_proxy_pass psql -h localhost -p 5433 -U ra_proxy -d ra_proxydb -c 'SELECT 1;'

# Test Redis
docker compose exec redis redis-cli ping
```

## Configuration

### Environment Variables

Edit `docker-compose.yml`:

**ra-web:**
```yaml
environment:
  - RUST_LOG=debug           # Logging level
  - ROCKET_PORT=8000         # Server port
  - DATABASE_URL=...         # PostgreSQL connection
  - REDIS_URL=...            # Redis connection
```

**postgres-ra-proxy:**
```yaml
environment:
  - RA_PROXY_PORT=8001              # Proxy API port
  - RA_PROXY_LOG_LEVEL=info         # Logging level
  - RA_PROXY_COMPARE_PLANS=true     # Enable plan comparison
  - RA_PROXY_INJECT_PLANS=false     # Inject Ra plans
```

### Resource Limits

Add to `docker-compose.yml`:
```yaml
services:
  ra-web:
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
        reservations:
          cpus: '0.5'
          memory: 512M
```

### Volumes

Persistent data locations:
```bash
# List volumes
docker volume ls | grep ra

# Inspect volume
docker volume inspect ra_pg-ra-extension-data

# Backup volume
docker run --rm -v ra_pg-ra-extension-data:/data -v $(pwd):/backup \
  alpine tar czf /backup/pg-data-backup.tar.gz /data

# Restore volume
docker run --rm -v ra_pg-ra-extension-data:/data -v $(pwd):/backup \
  alpine tar xzf /backup/pg-data-backup.tar.gz -C /
```

## Monitoring

### Health Checks

All services expose health endpoints:
```bash
# Check all services
docker compose ps

# Individual health checks
curl http://localhost:3000/health    # docs
curl http://localhost:8000/health    # ra-web
curl http://localhost:8001/health    # ra-proxy

# PostgreSQL health
docker compose exec postgres-ra-extension pg_isready
docker compose exec postgres-ra-proxy pg_isready
```

### Logs

```bash
# All services
docker compose logs -f

# Specific service
docker compose logs -f ra-web
docker compose logs -f postgres-ra-proxy

# Last 100 lines
docker compose logs --tail=100 ra-web

# Since timestamp
docker compose logs --since 2024-04-02T10:00:00 ra-web
```

### Metrics

```bash
# Container stats
docker stats

# Service-specific stats
docker stats ra_ra-web_1

# Resource usage
docker system df
```

## Troubleshooting

### Service won't start

```bash
# Check logs
docker compose logs <service>

# Check container status
docker compose ps

# Restart service
docker compose restart <service>

# Rebuild and restart
docker compose build <service>
docker compose up -d <service>
```

### Port conflicts

```bash
# Check what's using ports
lsof -i :3000
lsof -i :8000
lsof -i :5432

# Change ports in docker-compose.yml
ports:
  - "3001:80"    # Map to different host port
```

### Out of disk space

```bash
# Check usage
docker system df

# Clean up
docker system prune -a --volumes

# Remove specific resources
docker volume rm <volume>
docker image rm <image>
```

### Performance issues

```bash
# Check resource usage
docker stats

# Increase resources in docker-compose.yml
deploy:
  resources:
    limits:
      cpus: '4'
      memory: 4G

# Optimize PostgreSQL
# Edit postgresql.conf in container or volume
shared_buffers = 512MB
effective_cache_size = 2GB
```

### Database connection issues

```bash
# Test connection
psql -h localhost -p 5432 -U ra_test -d ra_testdb

# Check PostgreSQL logs
docker compose logs postgres-ra-extension

# Verify network
docker network inspect ra_ra-network

# Test from another container
docker compose exec ra-web sh
wget -O- http://postgres-ra-extension:5432
```

## Production Deployment

### Security Hardening

1. **Change default passwords:**
```yaml
environment:
  POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}  # Use secrets
```

2. **Enable TLS:**
```yaml
environment:
  - ROCKET_TLS_CERTS=/etc/ssl/certs/cert.pem
  - ROCKET_TLS_KEY=/etc/ssl/private/key.pem
```

3. **Network isolation:**
```yaml
networks:
  ra-network:
    internal: true  # No external access
  public:
    # Only expose necessary services
```

4. **Read-only filesystems:**
```yaml
services:
  ra-web:
    read_only: true
    tmpfs:
      - /tmp
```

### Backup Strategy

```bash
# Automated backup script
#!/bin/bash
BACKUP_DIR=/backups/$(date +%Y%m%d)
mkdir -p $BACKUP_DIR

# Backup PostgreSQL
docker compose exec -T postgres-ra-extension \
  pg_dumpall -U ra_test > $BACKUP_DIR/postgres.sql

# Backup Redis
docker compose exec -T redis redis-cli SAVE
docker cp ra_redis_1:/data/dump.rdb $BACKUP_DIR/redis.rdb

# Backup volumes
docker run --rm -v ra_pg-ra-extension-data:/data -v $BACKUP_DIR:/backup \
  alpine tar czf /backup/pg-data.tar.gz /data
```

### High Availability

Use Docker Swarm or Kubernetes for production:

**Docker Swarm:**
```bash
# Initialize swarm
docker swarm init

# Deploy stack
docker stack deploy -c docker-compose.yml ra

# Scale services
docker service scale ra_ra-web=3
```

**Kubernetes:**
```bash
# Convert to Kubernetes manifests
kompose convert -f docker-compose.yml

# Apply to cluster
kubectl apply -f *.yaml
```

## References

- [Docker Compose Documentation](https://docs.docker.com/compose/)
- [PostgreSQL Docker Official Image](https://hub.docker.com/_/postgres)
- [Rust Docker Best Practices](https://docs.docker.com/language/rust/)
- [cargo-chef for Docker Builds](https://github.com/LukeMathWalker/cargo-chef)
- [pgrx PostgreSQL Extensions](https://github.com/pgcentralfoundation/pgrx)
- [VitePress Documentation](https://vitepress.dev/)

## Support

For issues or questions:
- Open an issue on GitHub
- Check existing documentation in `docker/README.md`
- Review logs with `docker compose logs`

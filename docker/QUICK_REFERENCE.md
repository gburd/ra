# Docker Quick Reference Card

Quick reference for common Docker operations in the Ra project.

## One-Line Commands

```bash
# Build everything
docker compose build

# Start everything
docker compose up -d

# Test everything
./scripts/docker-test.sh

# Stop everything
docker compose down

# Nuke everything
docker compose down -v && docker system prune -a -f
```

## Service URLs

| Service | URL | Purpose |
|---------|-----|---------|
| Documentation | http://localhost:3000 | VitePress docs site |
| Ra Web API | http://localhost:8000 | REST API server |
| Ra Web Health | http://localhost:8000/health | Health check |
| PostgreSQL Ext | postgresql://ra_test:ra_test_pass@localhost:5432/ra_testdb | PG16 + Ra extension |
| PostgreSQL Proxy | postgresql://ra_proxy:ra_proxy_pass@localhost:5433/ra_proxydb | PG19 + Ra proxy |
| Proxy API | http://localhost:8001 | Proxy metrics |
| Redis | redis://localhost:6379 | Cache |
| PostgreSQL 15 | postgresql://test_user:test_pass@localhost:5415/test_db | Test DB |
| PostgreSQL 16 | postgresql://test_user:test_pass@localhost:5416/test_db | Test DB |
| MySQL 8 | mysql://test_user:test_pass@localhost:3306/test_db | Test DB |
| MariaDB | mysql://test_user:test_pass@localhost:3307/test_db | Test DB |
| DuckDB | http://localhost:8080 | Test DB |

## Build Commands

```bash
# Build specific services
docker compose build docs
docker compose build ra-web
docker compose build postgres-ra-extension
docker compose build postgres-ra-proxy

# Force rebuild (no cache)
docker compose build --no-cache

# Build with scripts
./scripts/docker-build.sh all
./scripts/docker-build.sh core
./scripts/docker-build.sh postgres
```

## Start/Stop Commands

```bash
# Start all services
docker compose up -d
./scripts/docker-up.sh all

# Start specific groups
./scripts/docker-up.sh core       # docs, ra-web, redis, pg-ext
./scripts/docker-up.sh databases  # test databases only
./scripts/docker-up.sh postgres   # both PostgreSQL services

# Stop services
docker compose down               # Stop, keep volumes
docker compose down -v            # Stop, remove volumes

# Restart services
docker compose restart
docker compose restart ra-web     # Restart specific service
```

## Logs and Monitoring

```bash
# View all logs
docker compose logs -f

# View specific service logs
docker compose logs -f ra-web
docker compose logs -f postgres-ra-proxy

# Last N lines
docker compose logs --tail=100 ra-web

# Since timestamp
docker compose logs --since 2024-04-02T10:00:00

# Service status
docker compose ps

# Container stats
docker stats
docker stats --no-stream

# Health checks
curl http://localhost:3000/health
curl http://localhost:8000/health
curl http://localhost:8001/health
```

## Database Access

```bash
# PostgreSQL with Ra extension
PGPASSWORD=ra_test_pass psql -h localhost -p 5432 -U ra_test -d ra_testdb

# PostgreSQL 19 with proxy
PGPASSWORD=ra_proxy_pass psql -h localhost -p 5433 -U ra_proxy -d ra_proxydb

# Test databases
PGPASSWORD=test_pass psql -h localhost -p 5415 -U test_user -d test_db  # PG15
PGPASSWORD=test_pass psql -h localhost -p 5416 -U test_user -d test_db  # PG16
mysql -h localhost -P 3306 -u test_user -ptest_pass test_db             # MySQL
mysql -h localhost -P 3307 -u test_user -ptest_pass test_db             # MariaDB

# Redis
docker compose exec redis redis-cli
```

## Container Shell Access

```bash
# Enter container shell
docker compose exec ra-web sh
docker compose exec postgres-ra-extension bash
docker compose exec postgres-ra-proxy bash
docker compose exec redis sh

# Run one-off command
docker compose exec ra-web ls -la /app
docker compose exec postgres-ra-extension pg_config
```

## API Testing

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

# Explain query
curl -X POST http://localhost:8000/api/explain \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT * FROM users WHERE age > 25","engine":"duckdb","analyze":true}'

# List rules
curl http://localhost:8000/api/rules
```

## Volume Management

```bash
# List volumes
docker volume ls | grep ra

# Inspect volume
docker volume inspect ra_pg-ra-extension-data

# Backup volume
docker run --rm \
  -v ra_pg-ra-extension-data:/data \
  -v $(pwd):/backup \
  alpine tar czf /backup/backup.tar.gz /data

# Restore volume
docker run --rm \
  -v ra_pg-ra-extension-data:/data \
  -v $(pwd):/backup \
  alpine tar xzf /backup/backup.tar.gz -C /

# Remove volume (danger!)
docker volume rm ra_pg-ra-extension-data
```

## Network Management

```bash
# Inspect network
docker network inspect ra_ra-network

# List containers on network
docker network inspect ra_ra-network | grep Name

# Test connectivity
docker compose exec ra-web ping postgres-ra-extension
docker compose exec ra-web wget -O- http://redis:6379
```

## Troubleshooting

```bash
# Check what's using a port
lsof -i :8000
netstat -tuln | grep 8000

# See container resource usage
docker stats

# Check disk usage
docker system df

# View container details
docker inspect <container>

# Container logs since last restart
docker compose logs --since 5m

# Follow logs for multiple services
docker compose logs -f ra-web postgres-ra-proxy

# Check health status
docker compose ps
docker inspect <container> | grep -A 10 Health
```

## Cleanup Commands

```bash
# Stop and remove containers
docker compose down

# Stop and remove containers + volumes
docker compose down -v

# Remove all stopped containers
docker container prune

# Remove unused images
docker image prune -a

# Remove unused volumes
docker volume prune

# Remove unused networks
docker network prune

# Nuclear option (remove everything)
docker system prune -a -f --volumes

# Clean Ra project specifically
docker compose down -v
docker images | grep ra | awk '{print $3}' | xargs docker rmi -f
docker volume ls | grep ra | awk '{print $2}' | xargs docker volume rm
```

## Makefile Commands

If using `Makefile.docker`:

```bash
# Build
make -f Makefile.docker build
make -f Makefile.docker build-web
make -f Makefile.docker rebuild

# Start/Stop
make -f Makefile.docker up
make -f Makefile.docker up-core
make -f Makefile.docker down

# Monitor
make -f Makefile.docker logs
make -f Makefile.docker logs-web
make -f Makefile.docker status
make -f Makefile.docker health

# Test
make -f Makefile.docker test

# Database access
make -f Makefile.docker psql-ext
make -f Makefile.docker psql-proxy
make -f Makefile.docker exec-redis

# Maintenance
make -f Makefile.docker backup-pg
make -f Makefile.docker backup-redis
make -f Makefile.docker clean
```

## Environment Variables

Set in `docker-compose.yml`:

```yaml
# ra-web
RUST_LOG: debug|info|warn|error
ROCKET_PORT: 8000
DATABASE_URL: postgresql://...
REDIS_URL: redis://...

# postgres-ra-proxy
RA_PROXY_PORT: 8001
RA_PROXY_LOG_LEVEL: debug|info|warn|error
RA_PROXY_COMPARE_PLANS: true|false
RA_PROXY_INJECT_PLANS: true|false
```

## File Locations

```
/home/gburd/ws/ra/
├── docker-compose.yml              # Main compose file
├── Makefile.docker                 # Make targets
├── DOCKER_DEPLOYMENT.md            # Full deployment guide
├── DOCKER_INFRASTRUCTURE_SUMMARY.md # Implementation summary
├── .dockerignore                   # Build context ignore
│
├── docs/
│   ├── Dockerfile                  # Docs multi-stage build
│   ├── nginx.conf                  # Nginx config
│   └── .dockerignore
│
├── crates/ra-web/
│   └── Dockerfile                  # Ra-web multi-stage build
│
├── docker/
│   ├── README.md                   # Docker docs
│   ├── DEPLOYMENT_CHECKLIST.md     # Validation checklist
│   ├── QUICK_REFERENCE.md          # This file
│   ├── postgres-ra-extension.Dockerfile
│   ├── postgres-ra-proxy.Dockerfile
│   ├── postgres-ra-extension-init.sql
│   ├── start-ra-proxy.sh
│   ├── postgres-init.sql
│   └── mysql-init.sql
│
└── scripts/
    ├── docker-build.sh             # Build helper
    ├── docker-up.sh                # Start helper
    └── docker-test.sh              # Test helper
```

## Common Workflows

### First-Time Setup
```bash
chmod +x scripts/docker-*.sh docker/start-ra-proxy.sh
./scripts/docker-build.sh all
./scripts/docker-up.sh all
./scripts/docker-test.sh
```

### Development Cycle
```bash
# Make code changes
docker compose build ra-web
docker compose up -d ra-web
docker compose logs -f ra-web
```

### Full Restart
```bash
docker compose down
docker compose up -d
```

### Clean Slate
```bash
docker compose down -v
docker system prune -a -f
./scripts/docker-build.sh all
./scripts/docker-up.sh all
```

### Debug Issues
```bash
docker compose ps
docker compose logs <service>
docker compose exec <service> sh
docker inspect <container>
```

## Keyboard Shortcuts

When viewing logs with `-f`:
- `Ctrl+C` - Stop following logs
- `Ctrl+Z` - Suspend (not recommended)

When in container shell:
- `Ctrl+D` or `exit` - Exit shell
- `Ctrl+P, Ctrl+Q` - Detach without stopping container

## Tips and Tricks

1. **Faster rebuilds**: Use `docker compose build --parallel`
2. **View image layers**: `docker history <image>`
3. **Copy files from container**: `docker cp <container>:<path> <local-path>`
4. **Execute as different user**: `docker compose exec -u root <service> sh`
5. **See live stats**: `watch -n 1 docker stats --no-stream`
6. **JSON output**: `docker inspect <container> --format='{{json .State}}'`
7. **Cleanup cron job**: Add `0 2 * * 0 docker system prune -a -f` to crontab
8. **Log rotation**: Configure Docker daemon with `log-opts` in `/etc/docker/daemon.json`

## Emergency Procedures

### Service not responding
```bash
docker compose restart <service>
docker compose logs <service>
```

### Out of memory
```bash
docker system prune -a -f
docker volume prune -f
```

### Database corruption
```bash
docker compose down
docker volume rm ra_pg-ra-extension-data
docker compose up -d postgres-ra-extension
# Restore from backup
```

### Port conflict
```bash
# Find what's using port
lsof -i :8000
# Kill process or change port in docker-compose.yml
```

## Support Resources

- Full docs: `/home/gburd/ws/ra/DOCKER_DEPLOYMENT.md`
- Docker docs: `/home/gburd/ws/ra/docker/README.md`
- Checklist: `/home/gburd/ws/ra/docker/DEPLOYMENT_CHECKLIST.md`
- Summary: `/home/gburd/ws/ra/DOCKER_INFRASTRUCTURE_SUMMARY.md`

---

Keep this file handy for quick reference during development and deployment!

# Docker Deployment Checklist

Use this checklist to validate your Docker deployment of the Ra query optimizer.

## Pre-Deployment

### Prerequisites
- [ ] Docker 24.0+ installed
- [ ] Docker Compose 2.20+ installed
- [ ] At least 8GB RAM available
- [ ] At least 20GB disk space available
- [ ] Ports 3000, 5432, 5433, 6379, 8000, 8001 available
- [ ] Git repository cloned

### File Permissions
```bash
chmod +x scripts/docker-*.sh
chmod +x docker/start-ra-proxy.sh
```
- [ ] Scripts are executable

## Build Phase

### Build All Images
```bash
./scripts/docker-build.sh all
```
- [ ] docs image built successfully
- [ ] ra-web image built successfully
- [ ] postgres-ra-extension image built successfully
- [ ] postgres-ra-proxy image built successfully

### Verify Images
```bash
docker images | grep ra
```
- [ ] All images present in registry
- [ ] Image sizes reasonable (< 500MB each)
- [ ] No `<none>` dangling images

## Deployment Phase

### Start Services
```bash
./scripts/docker-up.sh all
```
- [ ] All services started
- [ ] No error messages in startup logs
- [ ] All containers in "Up" state

### Check Container Status
```bash
docker compose ps
```
- [ ] docs: healthy
- [ ] ra-web: healthy
- [ ] postgres-ra-extension: healthy
- [ ] postgres-ra-proxy: healthy
- [ ] redis: healthy
- [ ] postgres-15: healthy
- [ ] postgres-16: healthy
- [ ] mysql-8: healthy
- [ ] mariadb: healthy
- [ ] duckdb: healthy

### Check Logs
```bash
docker compose logs --tail=50
```
- [ ] No ERROR level messages
- [ ] Services report successful initialization
- [ ] Database connections established

## Service Validation

### Documentation Site
```bash
curl -f http://localhost:3000/health
open http://localhost:3000
```
- [ ] Health check returns 200 OK
- [ ] Site loads in browser
- [ ] Navigation works
- [ ] Search functionality works

### Ra Web API
```bash
# Health check
curl -f http://localhost:8000/health

# Optimize query
curl -X POST http://localhost:8000/api/optimize \
  -H "Content-Type: application/json" \
  -d '{"expr":{"Scan":{"table":"users"}}}'

# Translate SQL
curl -X POST http://localhost:8000/api/translate \
  -H "Content-Type: application/json" \
  -d '{"sql":"SELECT * FROM t","from":"pg","to":"mysql"}'
```
- [ ] Health endpoint returns "OK"
- [ ] Optimize endpoint returns optimized plan
- [ ] Translate endpoint returns translated SQL
- [ ] No error responses

### PostgreSQL with Ra Extension
```bash
PGPASSWORD=ra_test_pass psql -h localhost -p 5432 -U ra_test -d ra_testdb
```
SQL commands:
```sql
-- Check extension
\dx pg_ra_planner

-- Test query
SELECT 1;

-- Test optimization
EXPLAIN (ANALYZE) SELECT * FROM pg_class WHERE relname LIKE 'pg_%';
```
- [ ] Connection successful
- [ ] Extension loaded (or gracefully handles if not available)
- [ ] Queries execute successfully
- [ ] EXPLAIN shows plan details

### PostgreSQL 19 Proxy
```bash
PGPASSWORD=ra_proxy_pass psql -h localhost -p 5433 -U ra_proxy -d ra_proxydb
```
SQL commands:
```sql
SELECT version();
SELECT 1;
```
- [ ] Connection successful
- [ ] PostgreSQL 19 version reported
- [ ] Queries execute successfully

### Ra Proxy API
```bash
curl -f http://localhost:8001/health
```
- [ ] Proxy API responds
- [ ] Returns health status

### Redis
```bash
docker compose exec redis redis-cli ping
docker compose exec redis redis-cli INFO
```
- [ ] PING returns PONG
- [ ] Server info accessible
- [ ] AOF persistence enabled

### Test Databases
```bash
# PostgreSQL 15
PGPASSWORD=test_pass psql -h localhost -p 5415 -U test_user -d test_db -c "SELECT 1;"

# PostgreSQL 16
PGPASSWORD=test_pass psql -h localhost -p 5416 -U test_user -d test_db -c "SELECT 1;"

# MySQL 8
mysql -h localhost -P 3306 -u test_user -ptest_pass -e "SELECT 1;" test_db

# MariaDB
mysql -h localhost -P 3307 -u test_user -ptest_pass -e "SELECT 1;" test_db

# DuckDB
curl -f http://localhost:8080/
```
- [ ] PostgreSQL 15 connection successful
- [ ] PostgreSQL 16 connection successful
- [ ] MySQL 8 connection successful
- [ ] MariaDB connection successful
- [ ] DuckDB API accessible

## Integration Testing

### Run Test Suite
```bash
./scripts/docker-test.sh
```
- [ ] All health checks pass
- [ ] API tests pass
- [ ] Database connection tests pass
- [ ] Zero test failures

### Performance Check
```bash
docker stats --no-stream
```
- [ ] CPU usage reasonable (< 80%)
- [ ] Memory usage within limits
- [ ] No memory leaks observed

### Network Check
```bash
docker network inspect ra_ra-network
```
- [ ] All containers connected to network
- [ ] No network errors

### Volume Check
```bash
docker volume ls | grep ra
```
- [ ] All volumes created
- [ ] No orphaned volumes

## Security Validation

### Non-Root User Check
```bash
docker compose exec ra-web whoami
docker compose exec redis whoami
```
- [ ] ra-web runs as non-root user
- [ ] Redis runs as redis user
- [ ] No services run as root

### Port Exposure Check
```bash
netstat -tuln | grep -E "3000|5432|5433|6379|8000|8001"
```
- [ ] Only intended ports exposed
- [ ] No unexpected open ports

### Health Check Configuration
```bash
docker compose config
```
- [ ] All services have health checks defined
- [ ] Health check intervals reasonable
- [ ] Start periods configured appropriately

## Documentation Validation

### README Files
- [ ] `/home/gburd/ws/ra/DOCKER_DEPLOYMENT.md` exists and is complete
- [ ] `/home/gburd/ws/ra/docker/README.md` exists and is complete
- [ ] `/home/gburd/ws/ra/DOCKER_INFRASTRUCTURE_SUMMARY.md` exists

### Configuration Files
- [ ] `docker-compose.yml` syntax valid
- [ ] All Dockerfiles follow multi-stage pattern
- [ ] `.dockerignore` files present and complete

### Scripts
- [ ] `scripts/docker-build.sh` works
- [ ] `scripts/docker-up.sh` works
- [ ] `scripts/docker-test.sh` works
- [ ] All scripts have proper error handling

## Production Readiness

### Security Hardening
- [ ] Default passwords changed
- [ ] Secrets configured (if applicable)
- [ ] TLS certificates prepared (if applicable)
- [ ] Firewall rules documented

### Monitoring
- [ ] Health check endpoints accessible
- [ ] Logging configured
- [ ] Log aggregation setup (if applicable)
- [ ] Metrics collection setup (if applicable)

### Backup Strategy
- [ ] Database backup script tested
- [ ] Redis backup script tested
- [ ] Backup schedule defined
- [ ] Backup restoration tested

### Resource Limits
- [ ] CPU limits set (if needed)
- [ ] Memory limits set (if needed)
- [ ] Disk usage monitoring enabled

### High Availability
- [ ] Load balancing configured (if applicable)
- [ ] Replica sets configured (if applicable)
- [ ] Failover tested (if applicable)

## Cleanup and Maintenance

### Cleanup Commands
```bash
# Stop services
docker compose down

# Remove volumes
docker compose down -v

# Prune system
docker system prune -a
```
- [ ] Cleanup commands tested
- [ ] Data persistence verified after restart

### Update Strategy
- [ ] Image update process documented
- [ ] Zero-downtime update plan (if applicable)
- [ ] Rollback procedure documented

## Sign-Off

### Development Environment
- [ ] All checklist items complete
- [ ] All tests passing
- [ ] Documentation reviewed
- [ ] Ready for development use

Date: _______________
Verified by: _______________

### Staging Environment
- [ ] All checklist items complete
- [ ] Performance tests passing
- [ ] Security scan complete
- [ ] Ready for staging deployment

Date: _______________
Verified by: _______________

### Production Environment
- [ ] All checklist items complete
- [ ] Load tests passing
- [ ] Security audit complete
- [ ] Monitoring configured
- [ ] Backup strategy implemented
- [ ] Disaster recovery plan in place
- [ ] Ready for production deployment

Date: _______________
Verified by: _______________

## Troubleshooting Reference

### Common Issues

| Issue | Command | Expected Result |
|-------|---------|-----------------|
| Service not starting | `docker compose logs <service>` | Shows error details |
| Port conflict | `lsof -i :<port>` | Shows what's using port |
| Out of memory | `docker stats` | Shows memory usage |
| Container unhealthy | `docker inspect <container>` | Shows health check failures |
| Network issues | `docker network inspect ra_ra-network` | Shows network config |

### Emergency Commands

```bash
# Stop everything immediately
docker compose down

# Force remove containers
docker compose rm -f -s -v

# Clean everything
docker system prune -a -f --volumes

# Restart from scratch
./scripts/docker-build.sh all
./scripts/docker-up.sh all
```

## Notes

Additional deployment notes:
_________________________________________________________________
_________________________________________________________________
_________________________________________________________________
_________________________________________________________________

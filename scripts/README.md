# Deployment Scripts

Scripts for launching the RA Web Explorer in various environments.

## Available Scripts

### `docker-run.sh`

Builds and runs the web explorer in a single Docker container.

**Usage:**
```bash
./scripts/docker-run.sh
```

**What it does:**
1. Builds Docker image from `Dockerfile`
2. Runs container on port 8000
3. Mounts rules directory for live updates
4. Displays logs in terminal

**Access:** http://localhost:8000

**Stop:** Press Ctrl+C

---

### `docker-compose-up.sh`

Launches the web explorer using Docker Compose (better for development).

**Usage:**
```bash
./scripts/docker-compose-up.sh
```

**What it does:**
1. Builds Docker image via `docker-compose.yml`
2. Starts service with health checks
3. Auto-restarts on failure
4. Displays logs in terminal

**Access:** http://localhost:8000

**Stop:** Press Ctrl+C

**Background mode:**
```bash
docker compose up -d
docker compose down  # to stop
```

---

### `deploy-fly.sh`

Deploys the web explorer to Fly.io cloud platform.

**Prerequisites:**
1. Install flyctl: `brew install flyctl` (macOS)
2. Create account: `flyctl auth login`

**Usage:**
```bash
./scripts/deploy-fly.sh
```

**What it does:**
1. Checks if flyctl is installed
2. Verifies you're logged in
3. Creates Fly.io app (first time only)
4. Builds and deploys Docker image
5. Provides app URL

**Access:** https://ra-explorer.fly.dev

**Configuration:** Edit `fly.toml` to customize

---

### `run-tla.sh`

Runs TLA+ model checker to verify formal properties.

**Prerequisites:**
- Install TLA+ Toolbox or `tlc` command-line tool
- macOS: `brew install tla-plus-toolbox`

**Usage:**
```bash
./scripts/run-tla.sh
```

**What it does:**
1. Checks all TLA+ specifications in `tla/` directory
2. Runs TLC model checker on each
3. Verifies 22 correctness properties
4. Generates verification logs

See [`tla/README.md`](../tla/README.md) for details.

---

## Quick Reference

| Task | Command |
|------|---------|
| **Run locally (Docker)** | `./scripts/docker-run.sh` |
| **Run locally (Compose)** | `./scripts/docker-compose-up.sh` |
| **Deploy to cloud** | `./scripts/deploy-fly.sh` |
| **Verify correctness** | `./scripts/run-tla.sh` |
| **Generate rules index** | `./scripts/generate-index.sh` |

## Environment Variables

All scripts respect these environment variables:

- `RUST_LOG`: Log level (trace, debug, info, warn, error)
- `ROCKET_PORT`: HTTP server port (default: 8000)
- `ROCKET_ADDRESS`: Bind address (default: 0.0.0.0)
- `STATIC_DIR`: Frontend assets path (default: /app/static)

Example:
```bash
RUST_LOG=debug ./scripts/docker-run.sh
```

## Troubleshooting

### Docker build fails

```bash
# Clear Docker cache
docker builder prune

# Rebuild from scratch
docker build --no-cache -t ra-web .
```

### Port 8000 already in use

```bash
# Find and kill process using port 8000
lsof -ti:8000 | xargs kill -9

# Or use a different port
ROCKET_PORT=8001 ./scripts/docker-run.sh
```

### Fly.io deployment fails

```bash
# Check authentication
flyctl auth whoami

# Re-login if needed
flyctl auth login

# View deployment logs
flyctl logs
```

### TLA+ verification fails

```bash
# Install TLA+ tools
brew install tla-plus-toolbox  # macOS
apt-get install tlaplus         # Ubuntu

# Verify installation
tlc -help
```

## See Also

- [Deployment Guide](../docs/deployment.md) - Comprehensive deployment documentation
- [TLA+ Specifications](../tla/README.md) - Formal verification guide
- [Docker Compose File](../docker-compose.yml) - Service configuration
- [Fly.io Config](../fly.toml) - Cloud deployment settings

---

**Last Updated**: 2026-03-17

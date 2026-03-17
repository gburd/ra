# Deployment Guide

This guide covers deploying the RA Web Explorer in different environments.

## Quick Start

### Local Development (Docker)

**Option 1: Docker Run Script** (simplest)
```bash
./scripts/docker-run.sh
```

**Option 2: Docker Compose** (better for development)
```bash
./scripts/docker-compose-up.sh
```

**Option 3: Manual Docker**
```bash
docker build -t ra-web .
docker run -p 8000:8000 ra-web
```

Then open: http://localhost:8000

### Cloud Deployment (Fly.io)

```bash
./scripts/deploy-fly.sh
```

Or manually:
```bash
# First time only
flyctl auth login
flyctl apps create ra-explorer

# Deploy
flyctl deploy
```

## Deployment Options

### 1. Docker Container (Local or Any Cloud)

#### Build Image

```bash
docker build -t ra-web:latest .
```

The Dockerfile is a multi-stage build:
- **Stage 1**: Builds frontend (Node.js + pnpm)
- **Stage 2**: Builds backend (Rust + Cargo)
- **Stage 3**: Creates minimal runtime image (~200MB)

#### Run Container

```bash
docker run -d \
  --name ra-web \
  -p 8000:8000 \
  -e RUST_LOG=info \
  -v $(pwd)/rules:/app/rules:ro \
  ra-web:latest
```

#### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ROCKET_PORT` | 8000 | HTTP server port |
| `ROCKET_ADDRESS` | 0.0.0.0 | Bind address |
| `RUST_LOG` | info | Log level (trace, debug, info, warn, error) |
| `STATIC_DIR` | /app/static | Frontend static files path |

#### Volume Mounts

- `/app/rules`: Rule definitions (read-only recommended)
- `/app/static`: Frontend assets (built-in, no mount needed)

### 2. Docker Compose (Development)

Best for local development with hot-reload.

#### Start Services

```bash
# Foreground (see logs, Ctrl+C to stop)
docker compose up --build

# Background
docker compose up -d --build

# View logs
docker compose logs -f ra-web
```

#### Stop Services

```bash
# Graceful stop
docker compose down

# Force stop and remove volumes
docker compose down -v
```

#### Configuration

Edit `docker-compose.yml` to customize:
- Port mapping
- Environment variables
- Volume mounts
- Resource limits

### 3. Fly.io (Production Cloud Hosting)

Fly.io provides:
- Global edge deployment
- Auto-scaling (0-N machines)
- HTTPS by default
- Pay-per-use (free tier available)

#### Prerequisites

1. Install flyctl:
   ```bash
   # macOS
   brew install flyctl

   # Linux
   curl -L https://fly.io/install.sh | sh

   # Windows
   powershell -Command "iwr https://fly.io/install.ps1 -useb | iex"
   ```

2. Create account and log in:
   ```bash
   flyctl auth login
   ```

#### Initial Setup

```bash
# Create app (first time only)
flyctl apps create ra-explorer --org personal

# Or use the deployment script
./scripts/deploy-fly.sh
```

#### Deploy Updates

```bash
# Deploy current code
flyctl deploy

# Deploy specific branch
flyctl deploy --image-label $(git rev-parse --short HEAD)

# Deploy with different config
flyctl deploy --config fly.production.toml
```

#### Configuration (`fly.toml`)

```toml
app = "ra-explorer"
primary_region = "iad"  # Washington D.C.

[build]
  dockerfile = "Dockerfile"

[http_service]
  internal_port = 8000
  force_https = true
  auto_stop_machines = "stop"    # Stop when idle
  auto_start_machines = true     # Start on request
  min_machines_running = 0       # Scale to zero

  [http_service.concurrency]
    type = "connections"
    hard_limit = 100
    soft_limit = 80

[[vm]]
  memory = "512mb"
  cpu_kind = "shared"
  cpus = 1
```

#### Scaling

```bash
# View current scale
flyctl scale show

# Scale to fixed 2 machines
flyctl scale count 2

# Increase memory
flyctl scale memory 1024

# Set auto-scaling range
flyctl scale count 1-5
```

#### Monitoring

```bash
# View live logs
flyctl logs

# View app status
flyctl status

# View metrics
flyctl dashboard

# SSH into machine
flyctl ssh console
```

#### Regions

Available regions:
- `iad` - Washington D.C., USA (default)
- `lax` - Los Angeles, USA
- `ams` - Amsterdam, Netherlands
- `nrt` - Tokyo, Japan
- `syd` - Sydney, Australia

Add regions:
```bash
flyctl regions add lax ams
flyctl regions list
```

#### Custom Domain

```bash
# Add custom domain
flyctl certs create ra-optimizer.org

# Verify DNS
flyctl certs check ra-optimizer.org
```

Required DNS records:
```
CNAME ra-optimizer.org -> ra-explorer.fly.dev
```

### 4. Kubernetes

For large-scale deployments.

#### Create Deployment

```yaml
# k8s/deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ra-web
spec:
  replicas: 3
  selector:
    matchLabels:
      app: ra-web
  template:
    metadata:
      labels:
        app: ra-web
    spec:
      containers:
      - name: ra-web
        image: ra-web:latest
        ports:
        - containerPort: 8000
        env:
        - name: RUST_LOG
          value: "info"
        - name: ROCKET_PORT
          value: "8000"
        resources:
          requests:
            memory: "256Mi"
            cpu: "250m"
          limits:
            memory: "512Mi"
            cpu: "500m"
        livenessProbe:
          httpGet:
            path: /health
            port: 8000
          initialDelaySeconds: 10
          periodSeconds: 30
---
apiVersion: v1
kind: Service
metadata:
  name: ra-web
spec:
  type: LoadBalancer
  ports:
  - port: 80
    targetPort: 8000
  selector:
    app: ra-web
```

#### Deploy

```bash
kubectl apply -f k8s/deployment.yaml
kubectl rollout status deployment/ra-web
kubectl get services ra-web
```

### 5. Bare Metal / VPS

For self-hosting on your own server.

#### Prerequisites

- Rust 1.75+ with cargo
- Node.js 22+ with pnpm
- nginx or caddy (for reverse proxy)

#### Build

```bash
# Build frontend
cd web
pnpm install
pnpm run build
cd ..

# Build backend
cargo build --release --bin ra-web

# Binary at: target/release/ra-web
```

#### Run with systemd

```ini
# /etc/systemd/system/ra-web.service
[Unit]
Description=RA Web Explorer
After=network.target

[Service]
Type=simple
User=ra
WorkingDirectory=/opt/ra
ExecStart=/opt/ra/ra-web
Restart=on-failure
RestartSec=5s

Environment="ROCKET_PORT=8000"
Environment="ROCKET_ADDRESS=127.0.0.1"
Environment="RUST_LOG=info"
Environment="STATIC_DIR=/opt/ra/static"

[Install]
WantedBy=multi-user.target
```

```bash
# Enable and start
sudo systemctl enable ra-web
sudo systemctl start ra-web
sudo systemctl status ra-web
```

#### Reverse Proxy (nginx)

```nginx
# /etc/nginx/sites-available/ra-web
server {
    listen 80;
    server_name ra-optimizer.org;

    location / {
        proxy_pass http://127.0.0.1:8000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_cache_bypass $http_upgrade;
    }
}
```

```bash
sudo ln -s /etc/nginx/sites-available/ra-web /etc/nginx/sites-enabled/
sudo nginx -t
sudo systemctl reload nginx
```

#### SSL with Certbot

```bash
sudo apt-get install certbot python3-certbot-nginx
sudo certbot --nginx -d ra-optimizer.org
```

### 6. Cloud Providers

#### AWS (ECS Fargate)

```bash
# Build and push to ECR
aws ecr create-repository --repository-name ra-web
docker build -t ra-web .
docker tag ra-web:latest $ECR_URI/ra-web:latest
docker push $ECR_URI/ra-web:latest

# Deploy to Fargate
aws ecs create-service \
  --cluster ra-cluster \
  --service-name ra-web \
  --task-definition ra-web \
  --desired-count 2 \
  --launch-type FARGATE
```

#### Google Cloud Run

```bash
# Build and deploy
gcloud builds submit --tag gcr.io/$PROJECT_ID/ra-web
gcloud run deploy ra-web \
  --image gcr.io/$PROJECT_ID/ra-web \
  --platform managed \
  --region us-central1 \
  --allow-unauthenticated
```

#### Azure Container Apps

```bash
# Build and push
az acr build --registry $ACR_NAME --image ra-web .

# Deploy
az containerapp create \
  --name ra-web \
  --resource-group ra-rg \
  --image $ACR_NAME.azurecr.io/ra-web \
  --target-port 8000 \
  --ingress external
```

## Performance Tuning

### Docker Build Optimization

**Cache Rust dependencies separately:**

```dockerfile
# Copy only Cargo files first
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p crates/ra-core/src && echo "" > crates/ra-core/src/lib.rs
RUN cargo build --release
RUN rm -rf crates/

# Then copy actual source
COPY crates/ crates/
RUN cargo build --release --bin ra-web
```

**Use build cache:**

```bash
docker build --cache-from ra-web:latest -t ra-web:latest .
```

### Resource Limits

Recommended minimum:
- **CPU**: 1 core
- **RAM**: 512MB
- **Disk**: 1GB

For production:
- **CPU**: 2 cores
- **RAM**: 1-2GB
- **Disk**: 2GB

### Horizontal Scaling

Run multiple instances behind a load balancer. The server is stateless, so horizontal scaling is straightforward.

```yaml
# docker-compose.yml with multiple replicas
services:
  ra-web:
    image: ra-web:latest
    deploy:
      replicas: 3
    ports:
      - "8000-8002:8000"
```

## Security

### HTTPS

Always use HTTPS in production:
- **Fly.io**: Automatic HTTPS
- **nginx**: Use certbot for Let's Encrypt
- **Cloud providers**: Use built-in SSL termination

### CORS Headers

The server sets appropriate CORS headers for WASM:
```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

### Rate Limiting

Built-in rate limiting:
- 100 requests per minute per IP
- Configurable via environment

### Firewall

Only expose port 8000 (HTTP):
```bash
ufw allow 8000/tcp
ufw enable
```

## Monitoring

### Health Check

```bash
curl http://localhost:8000/health
```

Expected response:
```json
{"status":"ok"}
```

### Metrics

View logs:
```bash
# Docker
docker logs -f ra-web

# Fly.io
flyctl logs

# systemd
journalctl -u ra-web -f
```

### Alerts

Set up alerts for:
- Health check failures
- High memory usage (>80%)
- High CPU usage (>80%)
- Request latency (>1s p99)

## Troubleshooting

### Container Won't Start

```bash
# Check logs
docker logs ra-web

# Common issues:
# - Port already in use: change ROCKET_PORT
# - Missing rules: mount ./rules volume
# - Permission denied: check file ownership
```

### High Memory Usage

```bash
# Increase memory limit
docker run -m 1g ra-web

# Or in fly.toml:
[[vm]]
  memory = "1024mb"
```

### Slow Build Times

```bash
# Use Rust build cache
docker build --build-arg CARGO_INCREMENTAL=1 .

# Or use sccache
ENV RUSTC_WRAPPER=sccache
```

### Frontend Not Loading

```bash
# Verify static files exist
docker exec ra-web ls /app/static

# Check STATIC_DIR environment variable
docker exec ra-web env | grep STATIC_DIR
```

## Cost Estimates

### Fly.io

- **Free tier**: 3 shared-cpu-1x VMs, 160GB bandwidth/month
- **Paid**: ~$0.0000022/second per VM ($5.70/month for 1 VM)
- **Bandwidth**: $0.02/GB after free tier

### AWS ECS Fargate

- **Compute**: $0.04/vCPU-hour + $0.004/GB-hour
- **1 task (0.5 vCPU, 1GB)**: ~$15/month
- **Load balancer**: $16/month

### Google Cloud Run

- **Free tier**: 2M requests/month
- **Paid**: $0.00002/request + $0.00001/GB-second
- **Typical**: $10-20/month for low traffic

### Self-Hosted VPS

- **DigitalOcean**: $6-12/month (1-2GB RAM)
- **Linode**: $5-10/month (1-2GB RAM)
- **Hetzner**: €4-8/month (2-4GB RAM)

## Next Steps

1. **Try it locally**: `./scripts/docker-run.sh`
2. **Deploy to Fly.io**: `./scripts/deploy-fly.sh`
3. **Set up monitoring**: Configure health checks and alerts
4. **Add custom domain**: Point DNS to your deployment
5. **Enable analytics**: Add tracking (optional)

## Support

- **Issues**: https://github.com/gregburd/ra/issues
- **Documentation**: https://ra-optimizer.org/docs
- **Fly.io Help**: https://community.fly.io
- **Docker Help**: https://docs.docker.com

---

**Last Updated**: 2026-03-17

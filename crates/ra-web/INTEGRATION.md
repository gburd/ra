# React Frontend + Rocket Backend Integration

This document describes the integration between the React frontend and Rocket backend for ra-web.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         Browser                              │
│  ┌──────────────────────────────────────────────────────┐   │
│  │            React App (http://localhost:5173)         │   │
│  │  - Monaco Editor for SQL                             │   │
│  │  - Material-UI components                            │   │
│  │  - Allotment resizable panes                         │   │
│  └──────────────────────────────────────────────────────┘   │
│                           │                                  │
│                           │ /api/* requests                  │
│                           ▼                                  │
│  ┌──────────────────────────────────────────────────────┐   │
│  │       Vite Dev Server Proxy (Development)            │   │
│  │       OR Direct (Production)                         │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│         Rocket Backend (http://localhost:8000)               │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  CORS Fairing (allows all origins)                   │   │
│  │  Rate Limiting (100 req/60s per IP)                  │   │
│  └──────────────────────────────────────────────────────┘   │
│                           │                                  │
│  ┌────────────┬──────────┴────────────┬─────────────────┐   │
│  │            │                       │                 │   │
│  │  API       │   React Frontend      │   Demo Pages    │   │
│  │  /api/*    │   /                   │   /demos/*.html │   │
│  │            │   frontend/dist/      │   static/       │   │
│  └────────────┴───────────────────────┴─────────────────┘   │
│                           │                                  │
│                           ▼                                  │
│  ┌──────────────────────────────────────────────────────┐   │
│  │        Ra Engine (Optimizer + Parser)                │   │
│  └──────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## File Structure

```
crates/ra-web/
├── frontend/                    # React frontend
│   ├── src/
│   │   ├── App.tsx             # Main app component
│   │   ├── components/         # React components
│   │   ├── hooks/              # Custom hooks
│   │   ├── types.ts            # TypeScript types
│   │   └── constants.ts        # Constants
│   ├── dist/                   # Build output (gitignored)
│   ├── package.json
│   ├── tsconfig.json
│   └── vite.config.ts          # Vite configuration
├── static/                     # Demo HTML pages
│   ├── index-selection.html
│   ├── hardware-plan.html
│   └── ...
├── src/
│   ├── main.rs                 # Rocket server entry point
│   ├── cors.rs                 # CORS middleware
│   └── api/                    # API endpoints
└── README.md                   # Updated documentation
```

## Development Workflow

### Frontend Development (with hot reload)

1. Start the Rocket backend:
   ```bash
   cargo run --bin ra-web
   # Backend starts on http://localhost:8000
   ```

2. Start the React dev server:
   ```bash
   cd crates/ra-web/frontend
   npm install
   npm run dev
   # Frontend starts on http://localhost:5173
   ```

3. Vite proxy configuration in `vite.config.ts` forwards `/api/*` requests to port 8000:
   ```typescript
   server: {
     port: 5173,
     proxy: {
       '/api': {
         target: 'http://localhost:8000',
         changeOrigin: true,
       },
     },
   }
   ```

4. Open http://localhost:5173 - changes to React code trigger instant hot reload

### Backend-Only Development

Run just the backend serving the built frontend:

```bash
cd crates/ra-web/frontend
npm run build

cd ../..
cargo run --bin ra-web
# Visit http://localhost:8000
```

## Production Deployment

### Local Production Build

```bash
# Build frontend
cd crates/ra-web/frontend
npm install
npm run build
# Output: dist/

# Build and run backend
cd ../..
cargo build --release --bin ra-web
./target/release/ra-web
```

### Docker Deployment

The Dockerfile uses multi-stage builds:

```dockerfile
# Stage 1: Build React frontend
FROM node:22-alpine AS frontend-build
WORKDIR /app
COPY crates/ra-web/frontend/package.json crates/ra-web/frontend/package-lock.json ./
RUN npm ci
COPY crates/ra-web/frontend/ ./
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:1.88-slim AS server-build
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY xtask/ xtask/
COPY rules/ rules/
RUN cargo build --release --bin ra-web

# Stage 3: Final runtime image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server-build /app/target/release/ra-web /app/ra-web
COPY --from=frontend-build /app/dist /app/frontend
COPY crates/ra-web/static /app/static
COPY rules/ /app/rules/

ENV ROCKET_PORT=8000
ENV ROCKET_ADDRESS=0.0.0.0
ENV FRONTEND_DIR=/app/frontend
ENV STATIC_DIR=/app/static

EXPOSE 8000
CMD ["/app/ra-web"]
```

Build and run:

```bash
docker build -t ra-web .
docker run -p 8000:8000 ra-web
```

## Environment Variables

| Variable         | Default                                    | Description                           |
|------------------|--------------------------------------------|---------------------------------------|
| `ROCKET_PORT`    | 8000                                       | Server port                           |
| `ROCKET_ADDRESS` | 0.0.0.0                                    | Bind address                          |
| `FRONTEND_DIR`   | `$CARGO_MANIFEST_DIR/frontend/dist`        | React frontend build directory        |
| `STATIC_DIR`     | `$CARGO_MANIFEST_DIR/static`               | Static demo pages directory           |

## CORS Configuration

The backend applies CORS headers to all responses via `cors.rs`:

```rust
response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
response.set_header(Header::new("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
response.set_header(Header::new("Access-Control-Allow-Headers", "Content-Type"));
```

This allows the Vite dev server (port 5173) to make requests to the backend (port 8000) during development.

Production deployments serve everything from the same origin, so CORS is not needed but doesn't hurt.

## Route Precedence

Rocket routes are mounted in this order:

1. **API routes** (`/api/*`) - highest priority
   - `/api/optimize`
   - `/api/execute`
   - `/api/visualize`
   - etc.

2. **Demo pages** (`/demos/*`)
   - Serves `crates/ra-web/static/*.html`

3. **React frontend** (`/`)
   - Serves `crates/ra-web/frontend/dist/`
   - Includes `/assets/*` for JS/CSS bundles

4. **SPA fallback** (rank 100)
   - Serves `frontend/dist/index.html` for any unmatched path
   - Enables client-side routing

## API Endpoints

The React frontend calls these backend endpoints:

- `POST /api/optimize` - Optimize a SQL query
- `POST /api/explain` - Get execution plan
- `POST /api/execute` - Execute SQL
- `POST /api/compare` - Compare across engines
- `POST /api/visualize` - Get plan visualization
- `GET /api/rules` - List optimizer rules
- `POST /api/share` - Create shareable link
- `GET /api/share/:id` - Load shared query

All endpoints return JSON and include CORS headers.

## Testing

### Frontend

```bash
cd crates/ra-web/frontend
npm run type-check   # TypeScript
npm run lint         # oxlint
npm run format       # oxfmt
```

### Backend

```bash
cargo test --package ra-web
```

Tests cover:
- All API endpoints
- CORS headers
- Rate limiting
- SPA fallback
- Share functionality

### Integration

Start both servers and test the full stack:

```bash
# Terminal 1: Backend
cargo run --bin ra-web

# Terminal 2: Frontend
cd crates/ra-web/frontend && npm run dev

# Terminal 3: Manual testing
curl http://localhost:8000/health
curl -X POST http://localhost:8000/api/optimize \
  -H "Content-Type: application/json" \
  -d '{"expr":{"Scan":{"table":"users"}}}'
```

## Troubleshooting

### Frontend build fails

Check Node version:
```bash
node --version  # Should be 22.x
npm --version
```

Clear cache and rebuild:
```bash
cd crates/ra-web/frontend
rm -rf node_modules dist
npm install
npm run build
```

### Backend can't find frontend files

Check environment variables:
```bash
FRONTEND_DIR=crates/ra-web/frontend/dist cargo run --bin ra-web
```

Verify build output exists:
```bash
ls -la crates/ra-web/frontend/dist/
# Should contain: index.html, assets/
```

### CORS errors in development

Vite proxy should forward API requests. Check `vite.config.ts`:
```typescript
proxy: {
  '/api': {
    target: 'http://localhost:8000',
    changeOrigin: true,
  },
}
```

### Port conflicts

Change ports if 8000 or 5173 are in use:
```bash
# Backend
ROCKET_PORT=8080 cargo run --bin ra-web

# Frontend (edit vite.config.ts)
server: { port: 5174 }
```

## Future Improvements

- [ ] Add WebSocket support for streaming optimization results
- [ ] Implement server-sent events for progress updates
- [ ] Add frontend tests with Vitest
- [ ] Implement query history in browser localStorage
- [ ] Add keyboard shortcuts for common operations
- [ ] Support multiple SQL editors (split panes)
- [ ] Add syntax highlighting for RelExpr output
- [ ] Implement plan diff view for before/after comparison

## References

- [Rocket Documentation](https://rocket.rs/)
- [React Documentation](https://react.dev/)
- [Vite Documentation](https://vite.dev/)
- [Monaco Editor](https://microsoft.github.io/monaco-editor/)
- [Material-UI](https://mui.com/)

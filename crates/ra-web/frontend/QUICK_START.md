# Quick Start Guide

## Prerequisites

- Node.js 20+ (you have 20.19.1)
- npm 10+ (you have 10.8.2)
- Running backend at http://localhost:8000

## Setup

```bash
cd crates/ra-web/frontend

# Install dependencies (first time only)
npm install
```

## Development

```bash
# Terminal 1: Start backend
cd /home/gburd/ws/ra
cargo run --bin ra-web

# Terminal 2: Start frontend dev server
cd /home/gburd/ws/ra/crates/ra-web/frontend
npm run dev
```

Open http://localhost:5173 in your browser.

## Build for Production

```bash
npm run build

# Output goes to: crates/ra-web/static/
```

Then run backend with:
```bash
STATIC_DIR=crates/ra-web/static cargo run --bin ra-web --release
```

## Features to Test

1. **SQL Editor**
   - Type a query: `SELECT * FROM employees WHERE department_id = 1;`
   - Press Ctrl+Enter to execute

2. **Engine Selection**
   - Change engine in dropdown (PostgreSQL, MySQL, DuckDB, SQLite)
   - Add more panels (up to 4) with "+" button

3. **EXPLAIN Modes**
   - Toggle between EXPLAIN and EXPLAIN ANALYZE
   - See different output formats

4. **Schemas**
   - Click schema button (table icon)
   - Browse HR or E-Commerce schemas
   - Click sample query to load it

5. **URL Sharing**
   - Click share button
   - Copy URL
   - Open in new tab to verify state restored

## Troubleshooting

**Port already in use:**
```bash
# Change port in vite.config.ts
server: { port: 5174 }
```

**Backend not responding:**
```bash
# Check backend is running on port 8000
curl http://localhost:8000/health
```

**TypeScript errors:**
```bash
npm run type-check
```

**Dependencies issues:**
```bash
rm -rf node_modules package-lock.json
npm install
```

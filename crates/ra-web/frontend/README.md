# RA Web Frontend

Godbolt.org-style SQL query optimizer interface with side-by-side engine comparison.

## Features

**MVP Phase 1 (Implemented):**

1. Split-pane interface with Monaco Editor for SQL input
2. Multi-engine comparison (PostgreSQL 15/16/17, MySQL 8.0/8.4, DuckDB, SQLite)
3. Query execution with EXPLAIN and EXPLAIN ANALYZE modes
4. Syntax-highlighted output with search and copy functionality
5. URL sharing with state encoding
6. Pre-defined schemas (HR, E-Commerce) with sample queries
7. Resizable panels with drag handles
8. Up to 4 concurrent engine comparisons

## Tech Stack

- React 18.3+ with TypeScript
- Monaco Editor (VS Code editor)
- Material-UI for components
- Allotment for split panes
- Vite for bundling

## Setup

```bash
cd crates/ra-web/frontend

# Install dependencies
npm install
```

## Development

```bash
# Start dev server (with proxy to backend)
npm run dev

# In another terminal, start the backend
cd ../..
cargo run --bin ra-web
```

The frontend will be available at `http://localhost:5173` and will proxy API requests to the backend at `http://localhost:8000`.

## Building

```bash
# Build for production
npm run build

# Output goes to crates/ra-web/static/
```

## Type Checking

```bash
npm run type-check
```

## Linting and Formatting

```bash
# Lint
npm run lint

# Format
npm run format
```

## Project Structure

```
src/
  types.ts              Type definitions
  constants.ts          Engine configs, schemas, sample queries
  main.tsx             React entry point
  App.tsx              Main application component
  components/
    Editor.tsx         Monaco editor wrapper with Ctrl+Enter support
    EngineSelector.tsx Engine dropdown component
    OutputPanel.tsx    Plan display with syntax highlighting
    Toolbar.tsx        Top toolbar with execute, share, schema buttons
    SchemaViewer.tsx   Schema and sample query browser
  hooks/
    useQueryExecution.ts  Query execution logic
  utils/
    api.ts             API client functions
    urlEncoding.ts     URL state encoding/decoding
```

## URL Sharing

Queries can be shared via URL. The URL format is `/p/:encoded_state` where the encoded state contains:

- SQL query text
- Selected engines for each panel
- EXPLAIN mode (explain vs analyze)

Example: `http://localhost:5173/p/eyJzIjoiU0VMRUNUICoiLCJlIjpbInBvc3RncmVzcWwtMTYiXSwibSI6ImV4cGxhaW4ifQ`

## API Endpoints Used

- `POST /api/explain` - Execute EXPLAIN query
  - Request: `{ "sql": "...", "engine": "postgresql", "analyze": false }`
  - Response: `{ "plan": "...", "engine": "postgresql", "analyzed": false }`

## Keyboard Shortcuts

- `Ctrl+Enter` or `Cmd+Enter` - Execute query

## Next Steps (Future Phases)

- Real database connections (PostgreSQL, MySQL, DuckDB)
- Visual plan tree rendering with D3.js
- Cost comparison charts
- Query history
- Plan diff view
- Export to various formats
- Authentication and saved queries

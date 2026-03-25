# ra-web-ui

SvelteKit frontend for the RA Query Explorer -- a SQL query parsing,
optimization, and visualization tool.

## Quick Start

```bash
# Install dependencies
pnpm install

# Start the development server (proxies /api/* to localhost:8000)
pnpm dev

# In a separate terminal, start the backend
cargo run --bin ra-web
```

Open `http://localhost:5173` in your browser.

## Production Build

```bash
pnpm build
```

Output goes to `build/`. Serve it from the ra-web backend:

```bash
STATIC_DIR=../ra-web-ui/build cargo run --bin ra-web
```

Or deploy the `build/` directory to any static hosting provider.

## Architecture

```
src/
  routes/
    +page.svelte          Main editor page
    +layout.svelte        App shell (header, navigation)
    +layout.ts            SPA mode (ssr=false, prerender=true)
  lib/
    api/
      client.ts           Typed API client for ra-web backend
      sqljsdb.ts          sql.js wrapper for in-browser SQLite
    components/
      Editor.svelte       Monaco Editor with SQL highlighting
      PlanTree.svelte      Recursive plan tree visualization
      ResultsTable.svelte  Query result table with timing
      Toolbar.svelte       Run/Visualize/Compare actions
      SchemaPanel.svelte   Schema template selector
      RulesPanel.svelte    Optimization rules list
      ComparePlans.svelte  Multi-optimizer plan comparison
      AstView.svelte       Parsed AST/RelExpr JSON view
      PipelineView.svelte  Optimization pipeline stages
      CostBreakdown.svelte Operator cost breakdown chart
  app.css                  CSS variables and global styles
  app.html                 HTML template
  app.d.ts                 TypeScript declarations
static/
  favicon.svg              RA logo
```

## Features

- **SQL Editor**: Monaco Editor with custom "ra-dark" theme,
  Ctrl+Enter to run, sample query loader
- **In-Browser Execution**: sql.js (SQLite WASM) for running queries
  without a backend
- **Plan Visualization**: Tree view with cost bars, row counts,
  operator coloring by type (scan=green, join=blue, sort=yellow)
- **AST View**: Parsed relational algebra expression as JSON
- **Pipeline View**: Side-by-side Logical -> Optimized plan stages
  with cost reduction percentages
- **Cost Breakdown**: Sorted operator costs with percentage bars
- **Plan Comparison**: Ra vs PostgreSQL vs MySQL vs DuckDB plans
- **Rules Panel**: Which optimization rules fired
- **Schema Templates**: Pre-built E-Commerce and Analytics schemas
- **Query History**: Persisted in localStorage

## API Endpoints

The frontend consumes these ra-web backend endpoints:

| Method | Path               | Description                          |
|--------|--------------------|--------------------------------------|
| POST   | /api/visualize     | Parse SQL, return positioned plan    |
| POST   | /api/compare-plans | Compare plans across optimizers      |
| POST   | /api/execute       | Execute SQL (sqlite/duckdb)          |
| POST   | /api/optimize      | Optimize a RelExpr                   |
| POST   | /api/translate     | Translate between SQL dialects       |
| GET    | /api/rules         | List available optimizer rules       |

## Development

```bash
# Type check
pnpm check

# Build for production
pnpm build

# Preview production build
pnpm preview
```

## Tech Stack

- SvelteKit 2.0 with Svelte 5 (runes)
- TypeScript (strict mode)
- Monaco Editor 0.52
- sql.js 1.12 (SQLite WASM)
- Vite 6
- Static adapter for deployment

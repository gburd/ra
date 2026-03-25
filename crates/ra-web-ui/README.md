# ra-web-ui

SvelteKit frontend for the RA Query Explorer -- a SQL query parsing,
optimization, and visualization tool similar to godbolt.org but for SQL.

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

Output goes to `build/` (544KB). Serve it from the ra-web backend:

```bash
STATIC_DIR=../ra-web-ui/build cargo run --bin ra-web --release
```

Or deploy the `build/` directory to any static hosting provider.

## Component Architecture

All components use Svelte 5 runes (`$state`, `$derived`, `$props`,
`$bindable`) for reactive state management. There is no global store --
all state lives in the main `+page.svelte` and flows down via props.

### Layout

```
+layout.svelte          App shell: header with logo and navigation
  +page.svelte          Main page: orchestrates all components
    Toolbar             Action buttons, sample query dropdown
    SchemaPanel         Sidebar: schema templates
    Editor              Monaco SQL editor
    [output tabs]
      ResultsTable      Query results with timing
      PlanTree          Plan tree + CostBreakdown + RulesPanel
      AstView           Parsed RelExpr JSON
      PipelineView      Optimization stages side by side
      ComparePlans      Multi-optimizer comparison grid
```

### Components

**Editor.svelte** -- Monaco Editor wrapper
- Custom "ra-dark" Catppuccin-inspired theme
- SQL language mode with keyword highlighting
- `Ctrl+Enter` keybinding to run queries
- `value` prop is `$bindable` for two-way binding
- Exposes `setValue()` method for programmatic updates
- Configures MonacoEnvironment for web worker loading

**PlanTree.svelte** -- Recursive plan tree visualization
- Self-referencing via `import PlanTreeSelf from "./PlanTree.svelte"`
- Color-codes operators: scan (green), join (blue), sort (yellow),
  filter (peach)
- Shows cost bar proportional to total plan cost
- Displays row count estimates and detail key-value pairs
- Hover highlights individual nodes

**ResultsTable.svelte** -- Query result display
- Sticky column headers
- Monospace font for data alignment
- NULL values styled in italic
- Row count and execution time in header bar

**Toolbar.svelte** -- Action bar
- Run (Ctrl+Enter), Visualize Plan, Compare Plans buttons
- Sample query dropdown with 5 pre-built queries
- `onsample` callback for loading queries into the editor
- Disabled state while queries are running

**SchemaPanel.svelte** -- Schema template selector
- Pre-built schemas: E-Commerce (4 tables), Analytics (2 tables)
- Apply button executes DDL against sql.js
- Schema SQL preview

**RulesPanel.svelte** -- Optimization rules list
- Numbered list of rules applied during optimization
- Hover highlighting

**ComparePlans.svelte** -- Multi-optimizer plan comparison
- Grid layout with one column per optimizer (Ra, PostgreSQL, MySQL, DuckDB)
- Summary bar showing cheapest optimizer with cost badges
- Each column contains a PlanTree instance

**AstView.svelte** -- Parsed AST display
- Formatted JSON representation of the VisualPlanNode tree
- Monospace scrollable view

**PipelineView.svelte** -- Optimization pipeline stages
- Side-by-side grid: Logical -> Optimized
- Header shows cost per stage with reduction percentage
- Each stage contains a PlanTree instance

**CostBreakdown.svelte** -- Operator cost analysis
- Sorted list of operators by cost (highest first)
- Percentage bars showing cost contribution
- Row count per operator
- Total cost header

### State Management

State is managed in `+page.svelte` using Svelte 5 runes:

```typescript
// Reactive state
let sql = $state("SELECT ...");          // Editor content
let running = $state(false);              // Loading indicator
let error = $state("");                   // Error message
let activeTab = $state<ActiveTab>("results");

// Query results
let queryResult = $state<QueryResult | null>(null);
let planResult = $state<VisualizeResponse | null>(null);
let compareResult = $state<ComparePlansResponse | null>(null);
let rulesApplied = $state<string[]>([]);
let astData = $state<unknown>(null);
let pipelineStages = $state<PipelineStage[]>([]);

// Persistent state
let history = $state<string[]>(loadHistory());  // localStorage
```

Data flow is top-down: `+page.svelte` holds all state, passes it
to child components via props, and receives events via callback
props (`onrun`, `onvisualize`, `onsample`, etc.).

## API Client

`src/lib/api/client.ts` provides typed functions for each backend
endpoint:

```typescript
import { visualize, comparePlans, execute, translate, listRules } from "$lib/api/client";

// Parse and optimize SQL, get plan tree
const result = await visualize("SELECT * FROM users WHERE age > 25");
// result.plan: VisualPlanNode tree
// result.total_cost: number
// result.rules_applied: string[]

// Compare across optimizers
const comparison = await comparePlans("SELECT ...");
// comparison.plans: Array<{ optimizer, plan, total_cost }>
// comparison.summary.cheapest: string
```

All functions use `fetch()` against `/api/*` paths, which the Vite
dev server proxies to `localhost:8000` (the ra-web backend).

`src/lib/api/sqljsdb.ts` wraps sql.js for in-browser SQLite:

```typescript
import { executeSQL, resetDb } from "$lib/api/sqljsdb";

// Execute SQL in browser (no backend needed)
const result = await executeSQL("SELECT 1 + 1 AS sum");
// result.columns: ["sum"]
// result.rows: [["2"]]
// result.timeMs: 0.5
```

## Styling System

All colors and tokens are defined as CSS custom properties in
`src/app.css`:

```css
:root {
  --bg-primary: #1e1e2e;     /* Main background */
  --bg-secondary: #181825;   /* Panels, sidebars */
  --bg-surface: #313244;     /* Cards, inputs */
  --bg-hover: #45475a;       /* Hover states */
  --text-primary: #cdd6f4;   /* Main text */
  --text-secondary: #a6adc8; /* Secondary text */
  --text-muted: #6c7086;     /* Muted/disabled text */
  --accent: #89b4fa;         /* Links, active states */
  --green: #a6e3a1;          /* Success, scan operators */
  --red: #f38ba8;            /* Errors */
  --yellow: #f9e2af;         /* Warnings, sort operators */
  --peach: #fab387;          /* Filter operators */
  --border: #45475a;         /* Borders */
  --font-mono: "JetBrains Mono", "Fira Code", monospace;
  --font-sans: "Inter", -apple-system, sans-serif;
  --radius: 8px;             /* Border radius */
  --radius-sm: 4px;          /* Small border radius */
}
```

Components use scoped `<style>` blocks and reference these variables.
The color palette is Catppuccin Mocha.

## Adding New Components

1. Create `src/lib/components/NewComponent.svelte`:

```svelte
<script lang="ts">
  interface Props {
    data: SomeType;
    onaction?: () => void;
  }

  let { data, onaction }: Props = $props();
</script>

<div class="container">
  <!-- markup -->
</div>

<style>
  .container {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
</style>
```

2. Import it in `+page.svelte` and add a tab or panel for it.
3. Add any new API types to `src/lib/api/client.ts`.

## Build Configuration

- **svelte.config.js**: Static adapter, outputs to `build/`
- **vite.config.ts**: SvelteKit plugin, Monaco pre-bundling,
  dev proxy to `localhost:8000`
- **tsconfig.json**: Strict mode, `noUncheckedIndexedAccess`,
  `verbatimModuleSyntax`, `isolatedModules`

## File Structure

```
src/
  routes/
    +page.svelte          Main editor page (orchestrator)
    +layout.svelte        App shell (header, navigation)
    +layout.ts            SPA mode (ssr=false, prerender=true)
  lib/
    api/
      client.ts           Typed API client for ra-web backend
      sqljsdb.ts          sql.js wrapper for in-browser SQLite
    components/
      Editor.svelte       Monaco Editor with SQL highlighting
      PlanTree.svelte     Recursive plan tree visualization
      ResultsTable.svelte Query result table with timing
      Toolbar.svelte      Run/Visualize/Compare actions
      SchemaPanel.svelte  Schema template selector
      RulesPanel.svelte   Optimization rules list
      ComparePlans.svelte Multi-optimizer plan comparison
      AstView.svelte      Parsed AST/RelExpr JSON view
      PipelineView.svelte Optimization pipeline stages
      CostBreakdown.svelte Operator cost breakdown
  app.css                 CSS variables and global styles
  app.html                HTML template
  app.d.ts                TypeScript declarations
static/
  favicon.svg             RA logo
```

## Development

```bash
pnpm check       # Type check (svelte-check + tsc)
pnpm build        # Production build
pnpm preview      # Preview production build locally
```

## Tech Stack

- SvelteKit 2.0 with Svelte 5 (runes)
- TypeScript (strict mode)
- Monaco Editor 0.52
- sql.js 1.12 (SQLite WASM)
- Vite 6
- Static adapter for deployment

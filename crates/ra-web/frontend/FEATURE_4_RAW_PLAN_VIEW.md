# Feature 4: Raw Plan View - Implementation Guide

## Overview

The Raw Plan View feature enhances the EXPLAIN output display with syntax highlighting, interactive search, copy functionality, cost display, and collapsible tree navigation.

## Components

### 1. PlanViewer (`src/components/PlanViewer.tsx`)

Main component for rendering EXPLAIN plans with advanced features.

**Features:**
- Syntax highlighting for SQL keywords and operations
- Color-coded operation types (Scan, Join, Aggregate, Filter)
- Inline cost estimates displayed as badges
- Timing information from EXPLAIN ANALYZE shown prominently
- Collapsible tree view for nested operations
- Expand/collapse all controls
- Search term highlighting with current match emphasis

**Props:**
```typescript
interface PlanViewerProps {
  planText: string;           // Raw EXPLAIN output
  searchTerm: string;         // Current search term
  currentMatchIndex: number;  // Index of current match to highlight
  onMatchCountChange: (count: number) => void;  // Callback when match count changes
}
```

**Usage:**
```tsx
<PlanViewer
  planText={explainOutput}
  searchTerm={searchTerm}
  currentMatchIndex={currentMatchIndex}
  onMatchCountChange={setMatchCount}
/>
```

### 2. SearchBar (`src/components/SearchBar.tsx`)

Search interface with navigation controls.

**Features:**
- Real-time search input
- Match counter display (e.g., "3 of 15")
- Previous/Next navigation buttons
- Keyboard shortcuts (Enter/Shift+Enter for next/prev, Esc to close)
- Close button

**Props:**
```typescript
interface SearchBarProps {
  onSearch: (term: string) => void;
  onNavigate: (direction: 'prev' | 'next') => void;
  onClose: () => void;
  matchCount: number;
  currentMatch: number;
}
```

### 3. OutputPanel (`src/components/OutputPanel.tsx`)

Updated to integrate PlanViewer and SearchBar.

**New Features:**
- Toggle search visibility with search icon button
- Copy to clipboard with success toast notification
- Seamless integration of enhanced plan viewing

## Utilities

### planParser (`src/utils/planParser.ts`)

Parsing and formatting utilities for EXPLAIN output.

**Key Functions:**

```typescript
// Parse cost estimate from EXPLAIN output
parseCost(line: string): CostEstimate | null

// Parse actual stats from EXPLAIN ANALYZE
parseActual(line: string): ActualStats | null

// Parse timing lines (Planning Time, Execution Time)
parseTimingLine(line: string): { label: string; value: number } | null

// Format milliseconds to human-readable time
formatTime(ms: number): string  // e.g., "123ms" → "0.12s", "0.5ms" → "500µs"

// Format large numbers with thousands separators
formatNumber(num: number): string  // e.g., 1234567 → "1,234,567"

// Calculate indent level from leading spaces
getIndentLevel(line: string): number

// Extract operation name from plan line
extractOperation(line: string): string | null

// Parse entire plan into structured nodes
parsePlan(planText: string): PlanNode[]

// Find all matches for search term
findMatches(planText: string, searchTerm: string): Match[]
```

**Data Types:**

```typescript
interface CostEstimate {
  startup: number;
  total: number;
  rows: number;
  width: number;
}

interface ActualStats {
  time: number;
  rows: number;
  loops: number;
}

interface PlanNode {
  line: string;
  indentLevel: number;
  operation: string | null;
  cost: CostEstimate | null;
  actual: ActualStats | null;
  highlight: boolean;
}
```

## Color Scheme

The PlanViewer uses a dark theme with VS Code-inspired syntax highlighting:

- **Background**: `#1e1e1e` (dark gray)
- **Text**: `#d4d4d4` (light gray)
- **Operations**: `#4ec9b0` (cyan) - Seq Scan, Hash Join, etc.
- **Keywords**: `#569cd6` (blue) - SELECT, FROM, WHERE, etc.
- **Metrics**: `#b5cea8` (green) - cost, rows, width values
- **Numbers**: `#b5cea8` (green)
- **Current Match**: `#ff9632` (orange) with bold weight
- **Other Matches**: `#ffd700` (gold)

## Badges

Cost and timing information are displayed as inline badges:

- **Cost Badge**: Green tint (`rgba(181, 206, 168, 0.2)`)
- **Time Badge**: Orange tint (`rgba(255, 150, 50, 0.2)`)
- **Timing Badge**: Blue tint (`rgba(86, 156, 214, 0.2)`)

## Keyboard Shortcuts

- **Enter**: Navigate to next match
- **Shift+Enter**: Navigate to previous match
- **Escape**: Close search bar

## User Flow

1. User runs EXPLAIN or EXPLAIN ANALYZE query
2. OutputPanel displays result with PlanViewer
3. User clicks search icon to open SearchBar
4. User types search term
5. Matches are highlighted in plan output
6. User navigates through matches with buttons or keyboard
7. User can collapse/expand operation nodes
8. User can copy entire plan to clipboard

## Testing

Tests are located in `src/utils/__tests__/planParser.test.ts`.

Run tests:
```bash
npm test
```

Run tests in watch mode:
```bash
npm test -- --watch
```

## Type Safety

All components are fully typed with TypeScript strict mode enabled:
- `strict: true`
- `noUncheckedIndexedAccess: true`
- `exactOptionalPropertyTypes: true`
- `noImplicitOverride: true`
- `noPropertyAccessFromIndexSignature: true`
- `verbatimModuleSyntax: true`

## Future Enhancements

Potential improvements for future iterations:

1. **Export Options**: Export plan as JSON, CSV, or formatted text
2. **Visual Plan**: Graphical tree visualization alongside text view
3. **Plan Comparison**: Side-by-side comparison of plans from different engines
4. **Performance Warnings**: Highlight expensive operations (e.g., sequential scans on large tables)
5. **Plan History**: Save and compare multiple plan executions
6. **Filtering**: Filter plan by operation type or cost threshold
7. **Custom Themes**: User-selectable color schemes

## Integration Points

The Raw Plan View integrates with:

- **EngineSelector**: Plans are engine-specific (PostgreSQL, MySQL, DuckDB, SQLite)
- **Toolbar**: Execute button triggers query and populates OutputPanel
- **API**: `/api/explain` endpoint returns plan text
- **State Management**: AppState stores panel configurations

## Dependencies

No external syntax highlighting libraries required. All highlighting is implemented using custom React components with inline styles.

Required MUI components:
- `Box`, `Paper`, `Typography`
- `IconButton`, `Tooltip`, `Chip`
- `TextField`, `Snackbar`, `Alert`
- `CircularProgress`

Required MUI icons:
- `ContentCopy`, `Search`
- `ExpandMore`, `ChevronRight`
- `UnfoldMore`, `UnfoldLess`
- `KeyboardArrowUp`, `KeyboardArrowDown`, `Close`

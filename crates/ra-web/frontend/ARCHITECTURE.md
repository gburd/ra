# RA Web Frontend Architecture

## Component Hierarchy

```
App (Root Component)
├── ThemeProvider (Material-UI dark theme)
├── CssBaseline (Material-UI CSS reset)
├── Toolbar
│   ├── Execute Button (with loading state)
│   ├── EXPLAIN Mode Toggle (explain / analyze)
│   ├── Add Panel Button (max 4 panels)
│   ├── Schema Button (opens SchemaViewer)
│   └── Share Button (opens ShareDialog)
├── Allotment (Split Panes)
│   ├── Left Pane: Editor
│   │   └── Monaco Editor (SQL syntax highlighting)
│   └── Right Pane: Output Panels
│       ├── Single Panel (when panels.length === 1)
│       │   └── OutputPanel
│       └── Multiple Panels (when panels.length > 1)
│           └── Allotment (Vertical)
│               ├── OutputPanel (Panel 1)
│               ├── OutputPanel (Panel 2)
│               ├── OutputPanel (Panel 3) [optional]
│               └── OutputPanel (Panel 4) [optional]
├── ShareDialog (Modal)
│   └── URL TextField + Copy Button
└── SchemaViewer (Modal)
    ├── Schema Tabs (HR / E-Commerce / TPC-H / Sakila / Blog)
    ├── Content Tabs (Tables / Sample Queries)
    ├── Tables Tab
    │   └── DDL Viewer (syntax highlighted)
    └── Sample Queries Tab
        └── Query List (click to load)
```

## Data Flow

```
┌─────────────────────────────────────────────────────────┐
│                      App State                           │
│  {                                                       │
│    sql: string,                                         │
│    explainMode: 'explain' | 'analyze',                  │
│    panels: OutputPanelState[]                           │
│  }                                                       │
└─────────────────────────────────────────────────────────┘
                          │
                          │ setState
                          │
        ┌─────────────────┼─────────────────┐
        │                 │                 │
        ▼                 ▼                 ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│   Editor     │  │   Toolbar    │  │ OutputPanel  │
│              │  │              │  │              │
│ onChange()   │  │ onExecute()  │  │ onEngine     │
│ onExecute()  │  │ onAddPanel() │  │ Change()     │
└──────────────┘  └──────────────┘  └──────────────┘
        │                 │                 │
        │                 │                 │
        └─────────────────┴─────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │  useQueryExecution()  │
              │                       │
              │  - executeSinglePanel │
              │  - executeAllPanels   │
              └───────────────────────┘
                          │
                          ▼
                ┌──────────────────┐
                │   API Client     │
                │                  │
                │  POST /api/      │
                │  explain         │
                └──────────────────┘
                          │
                          ▼
                ┌──────────────────┐
                │  Backend         │
                │  (Rocket)        │
                └──────────────────┘
```

## State Management Flow

### 1. Initial Load

```
URL → getStateFromUrl()
  ↓
Decode state from URL (if present)
  ↓
Initialize App state
  - sql: from URL or DEFAULT_SQL
  - explainMode: from URL or 'explain'
  - panels: from URL or [default panel]
```

### 2. User Edits SQL

```
User types in Editor
  ↓
Monaco onChange event
  ↓
handleSqlChange(newSql)
  ↓
setState({ ...state, sql: newSql })
  ↓
Editor re-renders with new value
```

### 3. User Executes Query

```
User clicks Execute (or Ctrl+Enter)
  ↓
handleExecute()
  ↓
executeAllPanels(panels, sql, explainMode)
  ↓
For each panel:
  - updatePanel(panelId, { loading: true })
  - API call: POST /api/explain
  - updatePanel(panelId, { output, loading: false })
  ↓
All panels update with results
```

### 4. User Adds Panel

```
User clicks Add Panel button
  ↓
handleAddPanel()
  ↓
Create new OutputPanelState
  ↓
setState({ ...state, panels: [...panels, newPanel] })
  ↓
Allotment re-renders with new pane
```

### 5. User Changes Engine

```
User selects engine in dropdown
  ↓
EngineSelector onChange
  ↓
onEngineChange(panelId, newEngine)
  ↓
updatePanel(panelId, { engine: newEngine })
  ↓
Panel re-renders with new engine label
```

### 6. User Shares Query

```
User clicks Share button
  ↓
handleShare()
  ↓
generateShareUrl(state)
  - Encode state to Base64
  - Create URL: /p/:encoded
  ↓
setShareUrl(url)
  ↓
Open ShareDialog
  ↓
User copies URL
  ↓
Paste URL in new tab
  ↓
getStateFromUrl() decodes state
  ↓
App loads with restored state
```

## Hook: useQueryExecution

```typescript
useQueryExecution(updatePanel) returns:
  - executeSinglePanel(panelId, sql, engine, explainMode)
  - executeAllPanels(panels, sql, explainMode)

Internal state:
  - abortControllers: Map<panelId, AbortController>

Flow:
1. Create AbortController for panel
2. Set panel loading state
3. Call API with timeout
4. On success: Update panel with output
5. On error: Update panel with error message
6. Cleanup: Remove AbortController
```

## URL State Encoding

```
State Object (TypeScript):
{
  s: string,      // SQL query
  e: Engine[],    // Array of engine IDs
  m: ExplainMode  // 'explain' or 'analyze'
}

Encoding Steps:
1. JSON.stringify(state)
2. btoa() to Base64
3. Replace URL-unsafe characters:
   - '+' → '-'
   - '/' → '_'
   - Remove '='
4. Result: URL-safe Base64 string

Example:
Input:  { s: "SELECT *", e: ["postgresql-16"], m: "explain" }
JSON:   {"s":"SELECT *","e":["postgresql-16"],"m":"explain"}
Base64: eyJzIjoiU0VMRUNUICoi...
URL:    /p/eyJzIjoiU0VMRUNUICoi...
```

## API Integration

### Request Format

```typescript
POST /api/explain
Content-Type: application/json

{
  "sql": "SELECT * FROM employees",
  "engine": "postgresql",  // Extract from Engine type
  "analyze": false         // From explainMode
}
```

### Response Format

```typescript
200 OK
Content-Type: application/json

{
  "plan": "QUERY PLAN\nSeq Scan...",
  "engine": "postgresql",
  "analyzed": false
}
```

### Error Handling

```typescript
try {
  const response = await fetch('/api/explain', { ... });
  if (!response.ok) {
    const error = await response.json();
    throw new ApiError(response.status, error.error);
  }
  return await response.json();
} catch (error) {
  // Display error in OutputPanel
  updatePanel(panelId, {
    loading: false,
    error: error.message,
    output: null
  });
}
```

## Theme Configuration

```typescript
const theme = createTheme({
  palette: {
    mode: 'dark',
    primary: {
      main: '#667eea',  // Purple accent
    },
  },
});

Applied to:
- Toolbar buttons
- Toggle buttons
- Icon buttons
- Text fields
- Dialogs
```

## Responsive Layout

```
Desktop (> 768px):
┌─────────────────────────────────────────────────────┐
│ Toolbar                                             │
├─────────────────────┬───────────────────────────────┤
│                     │ Output Panel 1                │
│                     ├───────────────────────────────┤
│   SQL Editor        │ Output Panel 2                │
│   (Monaco)          ├───────────────────────────────┤
│                     │ Output Panel 3                │
│                     ├───────────────────────────────┤
│                     │ Output Panel 4                │
└─────────────────────┴───────────────────────────────┘

Mobile (< 768px):
┌─────────────────────┐
│ Toolbar             │
├─────────────────────┤
│ SQL Editor          │
│ (Monaco)            │
├─────────────────────┤
│ Output Panel 1      │
├─────────────────────┤
│ Output Panel 2      │
└─────────────────────┘
(Stacked vertically)
```

## Performance Considerations

### Bundle Splitting

```
Main bundle:
  - React core
  - App components
  - Utils

Monaco Editor bundle:
  - Loaded separately (code-splitting)
  - ~3MB (largest component)
  - Cached after first load

Material-UI bundle:
  - Tree-shaken (only used components)
  - ~500KB
```

### Re-render Optimization

```typescript
// useCallback for stable function references
const handleExecute = useCallback(() => {
  executeAllPanels(state.panels, state.sql, state.explainMode);
}, [executeAllPanels, state.panels, state.sql, state.explainMode]);

// Prevents unnecessary re-renders of child components
```

### State Updates

```typescript
// Immutable state updates
setState(prevState => ({
  ...prevState,
  panels: prevState.panels.map(panel =>
    panel.id === panelId
      ? { ...panel, ...updates }
      : panel
  ),
}));

// Only affected panel re-renders
```

## Type Safety

### Strict TypeScript Configuration

```json
{
  "strict": true,
  "noUncheckedIndexedAccess": true,
  "exactOptionalPropertyTypes": true,
  "noImplicitOverride": true,
  "noPropertyAccessFromIndexSignature": true,
  "verbatimModuleSyntax": true
}
```

### Example: Array Access

```typescript
// Without noUncheckedIndexedAccess:
const panel = panels[0];  // Type: OutputPanelState

// With noUncheckedIndexedAccess:
const panel = panels[0];  // Type: OutputPanelState | undefined

// Forces null checking:
if (panel) {
  // Safe to use panel here
}
```

## Error Boundaries

Future enhancement - wrap components in error boundaries:

```typescript
<ErrorBoundary fallback={<ErrorDisplay />}>
  <App />
</ErrorBoundary>
```

## Accessibility

- Semantic HTML (button, input, dialog)
- ARIA labels on icon buttons
- Keyboard navigation (Tab, Shift+Tab)
- Keyboard shortcuts (Ctrl+Enter)
- Focus management in dialogs
- Screen reader compatible

## Testing Strategy (Future)

```typescript
// Unit tests (Vitest)
test('encodeState creates valid URL', () => {
  const state = { ... };
  const encoded = encodeState(state);
  const decoded = decodeState(encoded);
  expect(decoded).toEqual(state);
});

// Component tests (React Testing Library)
test('Execute button triggers query', async () => {
  render(<App />);
  const executeBtn = screen.getByText('Execute');
  fireEvent.click(executeBtn);
  await waitFor(() => {
    expect(screen.getByText(/QUERY PLAN/i)).toBeInTheDocument();
  });
});

// E2E tests (Playwright)
test('Full workflow', async ({ page }) => {
  await page.goto('/');
  await page.fill('[data-testid="sql-editor"]', 'SELECT 1');
  await page.click('[data-testid="execute-btn"]');
  await expect(page.locator('.output-panel')).toContainText('QUERY PLAN');
});
```

## Future Enhancements

### Phase 2: Visual Plan Trees

```
Add components:
  - PlanTreeView.tsx (D3.js tree layout)
  - PlanNode.tsx (tree node component)
  - PlanDetails.tsx (node detail tooltip)

Update OutputPanel:
  - Add view toggle (text / tree)
  - Parse EXPLAIN JSON output
  - Render D3 visualization
```

### Phase 3: Query History

```
Add components:
  - HistoryPanel.tsx (side panel)
  - HistoryItem.tsx (list item)

Add state:
  - history: QueryHistoryItem[]
  - Add to localStorage
  - Load on mount
```

### Phase 4: Cost Comparison

```
Add components:
  - ComparisonChart.tsx (Chart.js)
  - CostBreakdown.tsx (table view)

Update state:
  - Add cost metrics to OutputPanelState
  - Calculate total costs
  - Display comparison
```

---

**Architecture Status:** ✅ Complete for MVP Phase 1
**Next:** Implement Phase 2 visual plan trees with D3.js

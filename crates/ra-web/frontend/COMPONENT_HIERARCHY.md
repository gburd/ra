# Component Hierarchy - Feature 4: Raw Plan View

## Component Tree

```
App
└── OutputPanel (enhanced)
    ├── Toolbar
    │   ├── EngineSelector
    │   ├── SearchIconButton (new)
    │   └── CopyIconButton
    │
    ├── SearchBar (new - conditional)
    │   ├── TextField (search input)
    │   ├── Typography (match counter)
    │   ├── PrevButton
    │   ├── NextButton
    │   └── CloseButton
    │
    ├── PlanViewer (new)
    │   ├── ControlBar
    │   │   ├── ExpandAllButton
    │   │   └── CollapseAllButton
    │   │
    │   └── PlanContent
    │       └── PlanNode[] (for each line)
    │           ├── CollapseIcon (conditional)
    │           ├── HighlightedText
    │           ├── CostBadge (conditional)
    │           ├── TimeBadge (conditional)
    │           └── TimingBadge (conditional)
    │
    ├── LoadingState (conditional)
    │   ├── CircularProgress
    │   └── Typography
    │
    ├── ErrorState (conditional)
    │   └── Alert
    │
    ├── EmptyState (conditional)
    │   └── Typography
    │
    └── Snackbar (copy success toast)
```

## Data Flow

```
User Action → State Update → Component Re-render → UI Update

Example: Search Flow
1. User clicks search icon
   ├─> setSearchVisible(true)
   └─> SearchBar renders

2. User types "Filter"
   ├─> setSearchTerm("Filter")
   ├─> PlanViewer receives searchTerm prop
   ├─> findMatches() calculates positions
   ├─> setMatchCount() updates counter
   └─> highlightLine() emphasizes matches

3. User clicks Next
   ├─> handleNavigate('next')
   ├─> setCurrentMatchIndex((prev) => (prev + 1) % matchCount)
   ├─> PlanViewer receives new currentMatchIndex
   └─> match ref scrolls into view
```

## State Management

### OutputPanel State
```typescript
const [searchVisible, setSearchVisible] = useState(false);        // Show/hide SearchBar
const [searchTerm, setSearchTerm] = useState('');                 // Current search term
const [matchCount, setMatchCount] = useState(0);                  // Total matches found
const [currentMatchIndex, setCurrentMatchIndex] = useState(0);    // Current match position
const [copySuccess, setCopySuccess] = useState(false);            // Show copy toast
```

### PlanViewer State
```typescript
const [collapsed, setCollapsed] = useState<CollapsedState>({});   // { lineIndex: boolean }
const [allCollapsed, setAllCollapsed] = useState(false);          // Track expand/collapse all
```

### SearchBar State
```typescript
const [term, setTerm] = useState('');                             // Local search input
```

## Props Flow

### OutputPanel → PlanViewer
```typescript
<PlanViewer
  planText={panel.output}                    // Raw EXPLAIN output string
  searchTerm={searchTerm}                    // Current search term
  currentMatchIndex={currentMatchIndex}      // Which match to emphasize
  onMatchCountChange={setMatchCount}         // Callback to update match count
/>
```

### OutputPanel → SearchBar
```typescript
<SearchBar
  onSearch={handleSearch}                    // (term: string) => void
  onNavigate={handleNavigate}                // (direction: 'prev' | 'next') => void
  onClose={handleSearchClose}                // () => void
  matchCount={matchCount}                    // Total matches
  currentMatch={currentMatchIndex}           // Current position (0-indexed)
/>
```

## Utility Functions

### planParser.ts Exports
```typescript
// Parsing
parseCost(line: string): CostEstimate | null
parseActual(line: string): ActualStats | null
parseTimingLine(line: string): { label: string; value: number } | null
getIndentLevel(line: string): number
extractOperation(line: string): string | null
parsePlan(planText: string): PlanNode[]
findMatches(planText: string, searchTerm: string): Match[]

// Formatting
formatTime(ms: number): string
formatNumber(num: number): string
```

## Event Handlers

### OutputPanel
```typescript
handleCopy()                    // Copy plan text to clipboard
handleEngineChange(engine)      // Change selected engine
handleSearch(term)              // Update search term and reset index
handleNavigate(direction)       // Move to prev/next match
handleSearchClose()             // Hide SearchBar and clear search
```

### PlanViewer
```typescript
toggleCollapse(index)           // Toggle collapse state for node
expandAll()                     // Clear all collapse state
collapseAll()                   // Collapse all nodes with children
highlightLine(line, lineIndex)  // Apply syntax highlighting and search emphasis
formatLine(text)                // Apply color coding to text
isChildOfCollapsed(index)       // Check if node should be hidden
hasChildren(index)              // Check if node can be collapsed
```

## Refs

### PlanViewer Refs
```typescript
matchRefs: useRef<Array<HTMLSpanElement | null>>([])    // References to match elements
containerRef: useRef<HTMLDivElement>(null)              // Reference to scroll container
```

## Effects

### PlanViewer Effects
```typescript
// Update match count when matches change
useEffect(() => {
  onMatchCountChange(matches.length);
}, [matches.length, onMatchCountChange]);

// Scroll to current match when index changes
useEffect(() => {
  if (matches.length > 0 && currentMatchIndex >= 0) {
    matchRefs.current[currentMatchIndex]?.scrollIntoView({
      behavior: 'smooth',
      block: 'center'
    });
  }
}, [currentMatchIndex, matches]);
```

### SearchBar Effects
```typescript
// Trigger search when term changes
useEffect(() => {
  onSearch(term);
}, [term, onSearch]);
```

## Styling

### Theme
- Dark background (#1e1e1e)
- Light text (#d4d4d4)
- Monospace font (matches Monaco editor)

### Color Palette
```typescript
const colors = {
  operations: '#4ec9b0',      // Cyan - Seq Scan, Hash Join
  keywords: '#569cd6',        // Blue - SELECT, WHERE
  metrics: '#b5cea8',         // Green - cost, rows, width
  currentMatch: '#ff9632',    // Orange - emphasized match
  otherMatch: '#ffd700',      // Gold - other matches
  costBadge: 'rgba(181, 206, 168, 0.2)',
  timeBadge: 'rgba(255, 150, 50, 0.2)',
  timingBadge: 'rgba(86, 156, 214, 0.2)',
};
```

## Dependencies

### External (MUI)
- @mui/material: Box, Paper, Typography, TextField, IconButton, Tooltip, Chip, Snackbar, Alert, CircularProgress
- @mui/icons-material: ContentCopy, Search, ExpandMore, ChevronRight, UnfoldMore, UnfoldLess, KeyboardArrowUp, KeyboardArrowDown, Close

### Internal
- React: useState, useRef, useEffect
- Types: OutputPanelState, Engine (from types.ts)

## File Sizes

```
PlanViewer.tsx:     ~350 lines  (~12 KB)
SearchBar.tsx:      ~100 lines  (~3 KB)
planParser.ts:      ~250 lines  (~8 KB)
planParser.test.ts: ~150 lines  (~5 KB)
OutputPanel.tsx:    ~200 lines  (~7 KB) [updated]
```

## Performance Considerations

1. **Search**: O(n) regex search on plan text
2. **Collapse State**: O(1) lookup in object
3. **Match Navigation**: O(1) ref access
4. **Rendering**: O(n) where n = visible lines
5. **Syntax Highlighting**: Applied during render, no pre-processing

## Accessibility

- Keyboard navigation (Enter, Shift+Enter, Escape)
- Icon tooltips for screen readers
- Semantic HTML structure
- High contrast colors (WCAG AA)
- Focus management (auto-focus search input)

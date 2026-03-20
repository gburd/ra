# RFC 0027: Web-Based Query Comparison Interface

**Status:** Accepted
**Implemented:** Prior to 2026-03
**Commit:** Various

## Summary

Implemented a web-based interface for side-by-side query plan comparison, similar to Compiler Explorer (Godbolt). Users can compare execution plans across different optimizers, databases, and optimization levels to understand performance differences and optimization behavior.

## Motivation

Understanding query optimization requires:
- Visualizing execution plans
- Comparing different approaches
- Seeing optimization impact
- Sharing results with others

Existing tools lack:
- Side-by-side comparison
- Cross-database support
- Optimization rule tracking
- Shareable URLs

## Technical Design

### Architecture

**Frontend (Preact):**
- Lightweight React alternative
- Component-based UI
- Virtual DOM efficiency
- TypeScript for type safety

**Backend (Rust/WASM):**
- RA optimizer in browser
- SQLite/DuckDB execution
- Plan serialization
- Cost model evaluation

### UI Components

**Query Editor:**
- Syntax highlighting
- Auto-completion
- Schema awareness
- Error highlighting

**Plan Visualizer:**
- Tree representation
- Cost annotations
- Cardinality estimates
- Operator details

**Comparison Panes:**
- Synchronized scrolling
- Diff highlighting
- Cost delta display
- Rule application log

**Settings Panel:**
- Optimizer flags
- Database selection
- Cost model parameters
- Display preferences

### Data Flow

```
User Input (SQL)
    |
    v
Parse & Validate
    |
    v
Optimize (Multiple Configurations)
    |
    v
Generate Plans
    |
    v
Render Comparison
    |
    v
Interactive Visualization
```

### Plan Representation

JSON format for plans:
```json
{
  "type": "HashJoin",
  "cost": {
    "startup": 100.0,
    "total": 5000.0,
    "rows": 1000
  },
  "children": [
    {"type": "Scan", "table": "users"},
    {"type": "Scan", "table": "orders"}
  ],
  "annotations": {
    "join_type": "inner",
    "algorithm": "hash",
    "build_side": "left"
  }
}
```

### Shareable URLs

Encode state in URL:
```
https://ra-optimizer.io/?
  sql=SELECT...&
  left=postgres&
  right=ra-optimized&
  rules=all&
  share=abc123
```

## Implementation

### Frontend Structure

```
web/
├── src/
│   ├── components/
│   │   ├── QueryEditor.tsx
│   │   ├── PlanTree.tsx
│   │   ├── ComparisonPane.tsx
│   │   └── SettingsPanel.tsx
│   ├── lib/
│   │   ├── optimizer.ts
│   │   ├── planDiff.ts
│   │   └── storage.ts
│   ├── types.ts
│   └── main.tsx
├── package.json
└── vite.config.ts
```

### Key Features

**Plan Diffing:**
```typescript
function diffPlans(left: Plan, right: Plan): DiffResult {
  return {
    structural: compareStructure(left, right),
    costs: compareCosts(left, right),
    cardinality: compareCardinality(left, right),
    operators: compareOperators(left, right)
  };
}
```

**Optimization Tracking:**
```typescript
interface OptimizationStep {
  rule: string;
  before: Plan;
  after: Plan;
  improvement: number;
}
```

**Interactive Elements:**
- Hover for details
- Click to expand/collapse
- Drag to reorder panes
- Zoom for large plans

## Deployment

### Static Hosting

Build for CDN deployment:
```bash
npm run build
# Outputs to dist/
# Deploy to Cloudflare, Vercel, etc.
```

### Docker Container

```dockerfile
FROM nginx:alpine
COPY dist/ /usr/share/nginx/html
COPY nginx.conf /etc/nginx/nginx.conf
```

### Fly.io Configuration

```toml
app = "ra-optimizer"
primary_region = "sjc"

[http_service]
  internal_port = 80
  force_https = true
  auto_stop_machines = true
  auto_start_machines = true
```

## Usage Examples

### Basic Comparison

1. Enter SQL query
2. Select databases to compare
3. View side-by-side plans
4. Analyze differences

### Optimization Analysis

1. Enable rule tracking
2. Run optimization
3. Step through rule applications
4. See cumulative impact

### Performance Debugging

1. Paste slow query
2. Compare with/without indexes
3. Try different join orders
4. Export recommendations

## Testing

Frontend test coverage:
- Component unit tests
- Integration tests
- Visual regression tests
- Performance benchmarks
- Accessibility tests

## Performance

Optimization targets:
- Initial load: < 3s
- Plan rendering: < 100ms
- Optimization: < 500ms
- Memory usage: < 50MB

Achieved metrics:
- Lighthouse score: 95+
- First paint: < 1s
- Interactive: < 2s
- Bundle size: < 500KB

## Use Cases

**Development:**
- Query tuning
- Index impact analysis
- Join order experiments

**Education:**
- Teaching optimization
- Visualizing algorithms
- Interactive examples

**Debugging:**
- Production issues
- Performance regression
- Plan comparison

**Documentation:**
- Shareable examples
- Blog post illustrations
- Bug reports

## Browser Features

- PWA support
- Offline mode
- Local storage
- Share API
- Clipboard API

## Accessibility

- Keyboard navigation
- Screen reader support
- High contrast mode
- Focus indicators
- ARIA labels

## References

- Compiler Explorer (Godbolt)
- EXPLAIN Visualizer (Dalibo)
- PostgreSQL EXPLAIN
- Query Plan Visualization Research

## Future Work

- Real-time collaboration
- Query history tracking
- Performance profiling
- AI-powered suggestions
- Mobile optimization
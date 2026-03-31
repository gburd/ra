# Task #81: Interactive Plan Visualization - Architecture

## System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Browser (Client)                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │         plan-visualization.html                           │  │
│  ├───────────────────────────────────────────────────────────┤  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌─────────────────┐  │  │
│  │  │   D3.js     │  │  Control     │  │  Visualization  │  │  │
│  │  │  Tree       │  │  Panel       │  │  State Manager  │  │  │
│  │  │  Layout     │  │  (SQL Input) │  │  (Collapsed     │  │  │
│  │  └──────┬──────┘  └──────┬───────┘  │   Nodes)        │  │  │
│  │         │                │           └────────┬────────┘  │  │
│  │         └────────────────┴────────────────────┘           │  │
│  │                          │                                 │  │
│  │         ┌────────────────▼────────────────┐               │  │
│  │         │   Cost Breakdown Calculator     │               │  │
│  │         │   (CPU/IO/Memory/Network)       │               │  │
│  │         └─────────────────────────────────┘               │  │
│  └───────────────────────────────────────────────────────────┘  │
│                             │                                    │
│                             │ HTTP POST                          │
│                             ▼                                    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │
┌─────────────────────────────▼─────────────────────────────────────┐
│                      Rocket REST API Server                        │
├────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  POST /api/visualize                                         │ │
│  │  ┌────────────────────────────────────────────────────────┐ │ │
│  │  │  1. Parse SQL → RelExpr (ra-parser)                    │ │ │
│  │  │  2. Convert RelExpr to VisualPlanNode tree             │ │ │
│  │  │  3. Calculate costs and cardinality                    │ │ │
│  │  │  4. Add operator-specific details                      │ │ │
│  │  │  5. Return JSON with positioned tree                   │ │ │
│  │  └────────────────────────────────────────────────────────┘ │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐ │
│  │  POST /api/compare-plans                                     │ │
│  │  ┌────────────────────────────────────────────────────────┐ │ │
│  │  │  1. Parse SQL → RelExpr                                │ │ │
│  │  │  2. Generate Ra plan                                   │ │ │
│  │  │  3. Generate PostgreSQL-style plan                     │ │ │
│  │  │  4. Generate MySQL-style plan                          │ │ │
│  │  │  5. Generate DuckDB-style plan                         │ │ │
│  │  │  6. Calculate costs for each                           │ │ │
│  │  │  7. Identify cheapest optimizer                        │ │ │
│  │  │  8. Return comparison summary                          │ │ │
│  │  └────────────────────────────────────────────────────────┘ │ │
│  └──────────────────────────────────────────────────────────────┘ │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Core Components (ra-*)                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌─────────────┐  ┌──────────────┐  ┌─────────────────────┐   │
│  │ ra-parser   │  │ ra-core      │  │ ra-engine           │   │
│  │ (SQL→AST)   │  │ (RelExpr)    │  │ (Optimizer)         │   │
│  └─────────────┘  └──────────────┘  └─────────────────────┘   │
│                                                                   │
│  ┌─────────────┐  ┌──────────────┐  ┌─────────────────────┐   │
│  │ ra-stats    │  │ ra-hardware  │  │ ra-wasm             │   │
│  │ (Statistics)│  │ (HW Profile) │  │ (WASM bindings)     │   │
│  └─────────────┘  └──────────────┘  └─────────────────────┘   │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

## Data Flow

### Single Plan Visualization

```
User Input (SQL)
    │
    ├─► POST /api/visualize
    │       │
    │       ├─► sql_to_relexpr() → RelExpr
    │       │
    │       ├─► relexpr_to_visual() → VisualPlanNode tree
    │       │       │
    │       │       ├─► Pattern match on RelExpr variants
    │       │       ├─► Assign costs based on operator type
    │       │       ├─► Estimate cardinality
    │       │       ├─► Extract operator details
    │       │       └─► Recursively process children
    │       │
    │       └─► JSON Response {
    │               plan: VisualPlanNode,
    │               total_cost: f64,
    │               rules_applied: Vec<String>
    │           }
    │
    └─► Client receives JSON
            │
            ├─► D3.js creates hierarchy
            ├─► Tree layout calculates positions
            ├─► SVG renders nodes and links
            └─► User interacts:
                    ├─► Click → Toggle collapse
                    ├─► Hover → Show tooltip
                    └─► Zoom/Pan → Navigate
```

### Comparison Mode

```
User Input (SQL)
    │
    ├─► POST /api/compare-plans
    │       │
    │       ├─► Parse SQL once
    │       │
    │       ├─► build_plan_from_sql() → Ra plan
    │       ├─► build_pg_plan() → PostgreSQL plan
    │       ├─► build_mysql_plan() → MySQL plan
    │       └─► build_duckdb_plan() → DuckDB plan
    │               │
    │               └─► For each optimizer:
    │                       ├─► Generate VisualPlanNode tree
    │                       ├─► Calculate total cost
    │                       └─► Count nodes
    │
    │       └─► JSON Response {
    │               plans: [OptimizerPlan × 4],
    │               summary: {
    │                   cheapest: String,
    │                   costs: [OptimizerCost × 4]
    │               }
    │           }
    │
    └─► Client receives JSON
            │
            ├─► Render two side-by-side trees
            ├─► Display cost comparison
            ├─► Highlight cheapest optimizer
            └─► Show improvement metrics
```

## Component Interactions

### Frontend (plan-visualization.html)

```javascript
// State Management
state = {
    currentPlan: VisualPlanNode | null,
    originalPlan: VisualPlanNode | null,
    optimizedPlan: VisualPlanNode | null,
    comparisonMode: boolean,
    collapsedNodes: Set<string>
}

// Main Functions
visualizePlan(compare: boolean)
    ├─► Fetch API data
    ├─► Update state
    └─► Call render function

renderSinglePlan(data)
    ├─► renderStats()
    ├─► renderPlanTree()
    └─► renderCostBreakdown()

renderComparison(data)
    ├─► renderStats()
    ├─► renderPlanTree() × 2
    └─► Display side-by-side

renderPlanTree(svgId, rootNode)
    ├─► D3 hierarchy creation
    ├─► Tree layout application
    ├─► SVG rendering
    │   ├─► Links (paths)
    │   ├─► Nodes (groups)
    │   │   ├─► Rectangles (colored by type)
    │   │   ├─► Text (operator, cost, rows)
    │   │   └─► Collapse indicator
    │   └─► Event handlers
    │       ├─► click → toggleNode()
    │       ├─► mouseover → showTooltip()
    │       └─► mouseout → hideTooltip()
    └─► Apply zoom behavior
```

### Backend (visualize.rs)

```rust
// Data Structures
VisualPlanNode {
    id: String,
    operator_type: String,
    cost: f64,
    rows: u64,
    details: Vec<PlanDetail>,
    children: Vec<VisualPlanNode>,
    position: NodePosition
}

// Main Conversion Function
relexpr_to_visual(expr: &RelExpr) -> VisualPlanNode
    match expr {
        RelExpr::Scan { table, alias } =>
            // Create scan node with table details

        RelExpr::Filter { predicate, input } =>
            // Create filter node, recurse on input

        RelExpr::Join { left, right, .. } =>
            // Create join node, recurse on both sides

        RelExpr::Aggregate { input, .. } =>
            // Create aggregate node, recurse on input

        // ... handle all RelExpr variants
    }

// Helper Functions
sum_cost(node) → f64
    // Recursively sum costs across tree

count_nodes(node) → u32
    // Count total nodes in tree

extract_table(sql) → String
    // Parse table name from SQL
```

## Cost Calculation Algorithm

```
calculateCostBreakdown(node):
    breakdown = { cpu: 0, io: 0, memory: 0, network: 0 }

    traverse(node):
        opType = node.operator_type.toLowerCase()
        cost = node.cost

        if opType contains "scan" or "index":
            breakdown.io += cost × 0.7
            breakdown.cpu += cost × 0.2
            breakdown.memory += cost × 0.1

        else if opType contains "filter":
            breakdown.cpu += cost × 0.7
            breakdown.memory += cost × 0.2
            breakdown.io += cost × 0.1

        else if opType contains "join":
            breakdown.cpu += cost × 0.4
            breakdown.memory += cost × 0.4
            breakdown.io += cost × 0.2

        else if opType contains "aggregate" or "group":
            breakdown.cpu += cost × 0.5
            breakdown.memory += cost × 0.4
            breakdown.io += cost × 0.1

        else if opType contains "sort":
            breakdown.cpu += cost × 0.5
            breakdown.memory += cost × 0.4
            breakdown.io += cost × 0.1

        else:
            breakdown.cpu += cost × 0.5
            breakdown.memory += cost × 0.3
            breakdown.io += cost × 0.2

        for child in node.children:
            traverse(child)

    traverse(node)
    return breakdown
```

## Node Styling Algorithm

```
getNodeClass(operatorType):
    type = operatorType.toLowerCase()

    if "scan" in type: return "scan" → Blue (#e3f2fd)
    if "filter" in type: return "filter" → Yellow (#fff3cd)
    if "join" in type: return "join" → Red (#f8d7da)
    if "aggregate" in type: return "aggregate" → Green (#d4edda)
    if "sort" in type: return "sort" → Purple (#e7e3ff)

    return "" → White (default)
```

## Testing Strategy

```
┌────────────────────────────────────────────────────┐
│              Test Coverage                         │
├────────────────────────────────────────────────────┤
│                                                    │
│  Unit Tests (Backend)                              │
│  ├─► test_visualize_empty_sql                     │
│  ├─► test_visualize_valid                         │
│  ├─► test_compare_plans_empty_sql                 │
│  ├─► test_compare_plans_valid                     │
│  ├─► test_compare_plans_with_join                 │
│  ├─► test_plan_visualization_demo_in_list         │
│  ├─► test_plan_visualization_page_exists          │
│  ├─► test_visualize_with_complex_query            │
│  ├─► test_compare_plans_structure                 │
│  └─► test_visualize_cost_breakdown                │
│                                                    │
│  Integration Tests                                 │
│  ├─► HTML page loads and contains D3.js           │
│  ├─► Demo appears in /api/demos list              │
│  ├─► API returns valid JSON structure             │
│  ├─► Complex queries generate valid trees         │
│  └─► Comparison mode returns 4 optimizers         │
│                                                    │
│  Manual Testing                                    │
│  ├─► Interactive node collapse/expand             │
│  ├─► Tooltip display on hover                     │
│  ├─► Zoom and pan functionality                   │
│  ├─► Error handling for invalid SQL               │
│  └─► Hardware profile selection                   │
│                                                    │
└────────────────────────────────────────────────────┘
```

## Performance Characteristics

```
┌─────────────────────────────────────────────────┐
│  Component            │  Time Complexity        │
├───────────────────────┼─────────────────────────┤
│  SQL Parsing          │  O(n) - query length    │
│  RelExpr→Visual       │  O(m) - plan nodes      │
│  Cost Calculation     │  O(m) - plan nodes      │
│  D3 Tree Layout       │  O(m log m)             │
│  SVG Rendering        │  O(m)                   │
│  Collapse/Expand      │  O(m) - re-render       │
│  Tooltip Display      │  O(1)                   │
│  Zoom/Pan             │  O(1)                   │
└─────────────────────────────────────────────────┘

Typical plan sizes:
  - Simple queries: 5-20 nodes
  - Medium queries: 20-100 nodes
  - Complex queries: 100-500 nodes
  - Very large queries: 500+ nodes (use collapse)

Memory usage:
  - Client: ~1-5 MB per visualization
  - Server: Minimal (stateless API)
```

## Deployment Considerations

```
Production Checklist:
├─► D3.js CDN available and reliable
├─► CORS configured for API endpoints
├─► Rate limiting enabled (existing)
├─► Error handling for malformed SQL
├─► Browser compatibility tested
├─► Mobile responsiveness verified
├─► Performance profiling for large plans
└─► Documentation for end users
```

## Extension Points

```
Future Enhancements:
├─► Export/Import
│   ├─► Save as PNG/SVG
│   ├─► Export JSON
│   └─► Import saved plans
│
├─► Advanced Filtering
│   ├─► Hide operator types
│   ├─► Cost threshold filtering
│   └─► Cardinality highlighting
│
├─► Animation
│   ├─► Optimization replay
│   ├─► Step-through rules
│   └─► Smooth transitions
│
└─► Collaboration
    ├─► Plan annotations
    ├─► Share via URL
    └─► Version history
```

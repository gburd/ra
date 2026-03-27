# RA-Web Action Plan - Priority Order

## Phase 1: Fix Blockers (15-30 minutes)

### Task 1.1: Fix ra-engine Compilation Error
**File:** `crates/ra-engine/src/facts_context.rs:412`
**Issue:** `StorageFormat` import path incorrect
**Fix:**
```bash
cargo clean -p ra-engine
cargo build -p ra-engine
cargo test -p ra-engine
```

Expected: The file should already have the correct path `ra_core::facts::StorageFormat`, but the build cache may be stale.

### Task 1.2: Verify Web Server Builds and Runs
```bash
cargo build -p ra-web
cargo run -p ra-web
```

Expected: Server starts on http://localhost:8000

### Task 1.3: Manual Testing
Visit each demo page:
- http://localhost:8000/ (landing page)
- http://localhost:8000/staleness-impact.html
- http://localhost:8000/join-algorithm.html
- http://localhost:8000/hardware-plan.html
- http://localhost:8000/aggregation-strategy.html
- http://localhost:8000/index-selection.html

Test that "Analyze" button works and shows results.

## Phase 2: Connect HTML to API (1-1.5 hours)

### Task 2.1: Update index-selection.html
**Current:** Lines 320-361 use mock calculations
**Change:** Replace with API fetch

```javascript
// Replace line 320-361 with:
try {
    const response = await fetch('/api/demos/index-selection', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
            table_rows: tableSize,
            selectivity: selectivity,
            available_indexes: indexTypeSelect.value === 'none' ? [] : [indexTypeSelect.value],
            clustering_factor: clustering
        })
    });

    const result = await response.json();

    loading.classList.remove('visible');
    results.classList.add('visible');

    document.getElementById('decision').textContent = result.access_method;
    document.getElementById('reasoning').textContent = result.reasoning;
    document.getElementById('selectedRows').textContent = formatNumber(result.rows_accessed);
    document.getElementById('indexCost').textContent = result.estimated_cost.toFixed(0);
    // ... update remaining fields
} catch (error) {
    console.error('Error:', error);
    loading.classList.remove('visible');
    alert('Error analyzing index selection');
}
```

### Task 2.2: Update subquery-unnesting.html
Follow same pattern - replace mock with `/api/demos/subquery-unnesting` fetch

### Task 2.3: Update parallel-query.html
Replace mock with `/api/demos/parallel-query` fetch

### Task 2.4: Update gpu-offloading.html
Replace mock with `/api/demos/gpu-offloading` fetch

### Task 2.5: Update distributed-query.html
Replace mock with `/api/demos/distributed-query` fetch

### Task 2.6: Update cost-calibration.html
Replace mock with `/api/demos/cost-calibration` fetch

## Phase 3: Backend Optimizer Integration (1 hour)

### Task 3.1: Update demos2.rs - Index Selection
**File:** `crates/ra-web/src/api/demos2.rs`
**Function:** `demo_index_selection` (lines 35-100)

**Add imports:**
```rust
use ra_engine::Optimizer;
use ra_parser::sql_to_relexpr;
```

**Replace heuristic (line 42-88) with:**
```rust
// Create optimizer
let optimizer = Optimizer::new();

// Build test query with given selectivity
let sql = format!(
    "SELECT * FROM table WHERE id < {}",
    (request.table_rows as f64 * request.selectivity) as u64
);

// Parse and optimize
let plan = sql_to_relexpr(&sql).map_err(|e| {
    // Error handling
})?;

let optimized = optimizer.optimize(&plan).map_err(|e| {
    // Error handling
})?;

// Extract decision from optimized plan
let access_method = detect_scan_type(&optimized);
let cost = estimate_plan_cost(&optimized);
```

### Task 3.2: Update demos2.rs - Subquery Unnesting
Similar pattern for `demo_subquery_unnesting`

### Task 3.3: Update demos2.rs - Parallel Query
Similar pattern for `demo_parallel_query`

### Task 3.4: Update demos2.rs - GPU Offloading
Similar pattern for `demo_gpu_offloading`

### Task 3.5: Update demos2.rs - Distributed Query
Similar pattern for `demo_distributed_query`

### Task 3.6: Update demos2.rs - Cost Calibration
Similar pattern for `demo_cost_calibration`

## Phase 4: Plan Visualization (3 hours)

### Task 4.1: Add D3.js Plan Renderer (2 hours)
**Create:** `crates/ra-web/static/plan-viewer.html`

```html
<!DOCTYPE html>
<html>
<head>
    <title>Query Plan Viewer</title>
    <script src="https://d3js.org/d3.v7.min.js"></script>
</head>
<body>
    <div id="controls">
        <textarea id="sqlInput" placeholder="Enter SQL query..."></textarea>
        <button id="visualizeBtn">Visualize</button>
    </div>
    <svg id="plan"></svg>

    <script>
    async function visualizePlan() {
        const sql = document.getElementById('sqlInput').value;
        const response = await fetch('/api/visualize', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ sql })
        });

        const { plan } = await response.json();
        renderTree(plan);
    }

    function renderTree(node) {
        // D3.js tree layout
        const width = 1200;
        const height = 800;

        const svg = d3.select('#plan')
            .attr('width', width)
            .attr('height', height);

        const tree = d3.tree().size([height - 100, width - 200]);
        const root = d3.hierarchy(node, d => d.children);
        tree(root);

        // Draw links
        svg.selectAll('.link')
            .data(root.links())
            .enter().append('path')
            .attr('class', 'link')
            .attr('d', d3.linkHorizontal()
                .x(d => d.y)
                .y(d => d.x));

        // Draw nodes
        const nodes = svg.selectAll('.node')
            .data(root.descendants())
            .enter().append('g')
            .attr('class', 'node')
            .attr('transform', d => `translate(${d.y},${d.x})`);

        nodes.append('circle')
            .attr('r', 5);

        nodes.append('text')
            .attr('dy', 3)
            .attr('x', d => d.children ? -8 : 8)
            .text(d => d.data.operator_type);

        // Add tooltips with cost info
        nodes.append('title')
            .text(d => `${d.data.operator_type}\nCost: ${d.data.cost}\nRows: ${d.data.rows}`);
    }

    document.getElementById('visualizeBtn').addEventListener('click', visualizePlan);
    </script>
</body>
</html>
```

### Task 4.2: Add Visualization to Existing Demos (1 hour)
Update each demo HTML to include a plan visualization section:
```html
<div class="plan-section">
    <h3>Query Plan</h3>
    <svg id="planSvg"></svg>
</div>
```

Fetch plan after demo calculation:
```javascript
const planResponse = await fetch('/api/visualize', {
    method: 'POST',
    body: JSON.stringify({ sql: generatedSQL })
});
const { plan } = await planResponse.json();
renderPlan(plan);
```

## Phase 5: Testing & Polish (1-2 hours)

### Task 5.1: Integration Tests
```bash
cargo test -p ra-web --lib
```

### Task 5.2: Browser Testing
- [ ] Chrome (latest)
- [ ] Firefox (latest)
- [ ] Safari (latest)

Test each demo:
1. Load page
2. Adjust sliders/inputs
3. Click "Analyze"
4. Verify results display
5. Check console for errors

### Task 5.3: Mobile Testing
Test on:
- [ ] iOS Safari
- [ ] Android Chrome

Check:
- [ ] Layout responsive
- [ ] Sliders work with touch
- [ ] Text readable
- [ ] Buttons large enough

### Task 5.4: Performance Optimization
Add debouncing to sliders:
```javascript
let debounceTimer;
slider.addEventListener('input', () => {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
        updateDisplay();
    }, 300);
});
```

### Task 5.5: Error Handling
Add try-catch and user-friendly errors:
```javascript
try {
    const response = await fetch('/api/demos/...');
    if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    const result = await response.json();
    // ...
} catch (error) {
    loading.classList.remove('visible');
    alert(`Error: ${error.message}\nPlease check console for details.`);
    console.error('Demo error:', error);
}
```

## Phase 6: Documentation (30 minutes)

### Task 6.1: Update DEMO_INTEGRATION.md
Mark all tasks as complete, add deployment instructions.

### Task 6.2: Add README to crates/ra-web/
```markdown
# RA Web Server

Interactive demonstration server for the RA query optimizer.

## Quick Start

```bash
cargo run -p ra-web
```

Visit http://localhost:8000

## Demos

- Statistics Staleness Impact
- Hardware-Aware Planning
- Join Algorithm Selection
- Aggregation Strategies
- Index Selection
- Subquery Unnesting
- Parallel Query Execution
- GPU Offloading
- Distributed Planning
- Cost Calibration

## API Endpoints

See `src/api/` for endpoint documentation.
```

### Task 6.3: Add Comments to Complex Code
Add JSDoc comments to visualization functions, explain optimizer integration points.

## Deployment Checklist

- [ ] All compilation errors fixed
- [ ] All tests passing
- [ ] All 10 demos tested manually
- [ ] Browser compatibility verified
- [ ] Mobile responsiveness confirmed
- [ ] Error handling tested
- [ ] Documentation updated
- [ ] Performance acceptable (<2s response times)

## Time Tracking

| Phase | Estimated | Actual |
|-------|-----------|--------|
| Fix Blockers | 30 min | |
| Connect HTML to API | 1.5 hours | |
| Backend Optimizer Integration | 1 hour | |
| Plan Visualization | 3 hours | |
| Testing & Polish | 2 hours | |
| Documentation | 30 min | |
| **Total** | **8.5 hours** | |

## Notes

- WASM optimizer is built but not yet integrated client-side (can be post-launch feature)
- Visualize endpoint exists but frontend visualization needs D3.js implementation
- Some demos (first 4) already use ra_stats/ra_hardware, just need optimizer integration
- Test coverage is excellent (29 tests), main gap is end-to-end manual testing

## Next Steps

1. Start with Phase 1 (fix compilation error)
2. Verify server runs and demos are accessible
3. Move to Phase 2 (connect HTML to API) - highest impact
4. Phase 3 (optimizer integration) can be done in parallel with Phase 4
5. Phase 5 (testing) should be done incrementally after each phase
6. Phase 6 (docs) can be done anytime

Good luck! 🚀

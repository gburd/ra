# Comparison Components

This directory contains components for comparing query execution plans across multiple database engines.

## DiffView

A side-by-side visual comparison component that highlights differences between two query execution plans.

### Features

- Tree diff algorithm that identifies added, removed, and changed nodes
- Color-coded highlighting:
  - Green: nodes added in plan2
  - Red: nodes removed from plan1
  - Yellow: nodes with changed cost or row estimates (>10% difference)
  - Gray: unchanged nodes
- Side-by-side rendering with synchronized tree structure
- Expandable/collapsible tree nodes
- Click handlers for node highlighting in main views
- Percentage difference calculation for costs and rows
- Legend in toolbar showing color meanings

### Usage

```tsx
import { DiffView } from './components/comparison';
import type { ParsedPlan } from './types';

function PlanComparisonView({ plan1, plan2 }: { plan1: ParsedPlan; plan2: ParsedPlan }) {
  const handleNodeClick = (nodeId: string, planIndex: 1 | 2) => {
    console.log(`Clicked node ${nodeId} in plan ${planIndex}`);
    // Highlight the node in the corresponding panel
  };

  return (
    <DiffView
      plan1={plan1}
      plan2={plan2}
      onNodeClick={handleNodeClick}
    />
  );
}
```

### Props

- `plan1: ParsedPlan` - First plan to compare (left side)
- `plan2: ParsedPlan` - Second plan to compare (right side)
- `onNodeClick?: (nodeId: string, planIndex: 1 | 2) => void` - Optional click handler for node selection

### Diff Algorithm

The component uses a tree-based diff algorithm that:
1. Matches nodes by operation type and relation name
2. Traverses both trees in parallel
3. Detects changes based on cost and row estimate thresholds (10%)
4. Identifies added nodes (present only in plan2)
5. Identifies removed nodes (present only in plan1)
6. Preserves tree structure for easy visual comparison

### Styling

- Dark theme matching CostAnalysisView and other visualizations
- Material-UI Paper, Grid, and Chip components
- Responsive 50/50 grid layout
- Color-coded borders and backgrounds based on diff status
- Indentation based on tree depth
- Expand/collapse icons for tree navigation

## ComparisonTable

A statistical comparison table that displays key metrics side-by-side for multiple execution plans.

### Features

- Displays metrics for up to 4 engines simultaneously
- Highlights best (green) and worst (red) values for each metric
- Shows percentage of maximum cost for easy comparison
- Includes visual indicators for index usage
- Formatted values with K/M suffixes for large numbers

### Usage

```tsx
import { ComparisonTable } from './components/comparison';
import type { CostMetrics } from './types';

function ComparisonView() {
  const metrics: CostMetrics[] = [
    {
      totalCost: 100.5,
      totalRows: 1000,
      planDepth: 3,
      operationBreakdown: [
        // ... operation data
      ],
    },
    {
      totalCost: 85.3,
      totalRows: 950,
      planDepth: 2,
      operationBreakdown: [
        // ... operation data
      ],
    },
  ];

  const engineNames = ['PostgreSQL 15', 'MySQL 8.4'];

  return <ComparisonTable metrics={metrics} engineNames={engineNames} />;
}
```

### Props

- `metrics: CostMetrics[]` - Array of cost metrics from different engines
- `engineNames: string[]` - Array of engine names corresponding to the metrics

### Metrics Displayed

1. **Total Cost** - Overall execution cost (lower is better)
   - Shows raw value and percentage of maximum
2. **Estimated Rows** - Total rows processed (lower is better)
3. **Plan Depth** - Depth of the execution plan tree (lower is better)
4. **Scan Operations** - Count of scan operations (lower is better)
5. **Join Operations** - Count of join operations (lower is better)
6. **Sort Operations** - Count of sort operations (lower is better)
7. **Index Usage** - Whether indexes are used (Yes/No with icons)

### Styling

- Matches the dark theme of CostAnalysisView
- Uses Material-UI Table components
- Best values highlighted with green background (#064E3B)
- Worst values highlighted with red background (#7F1D1D)
- Percentage chips color-coded:
  - Green (<50%): #065F46
  - Yellow (50-80%): #92400E
  - Red (>80%): #7F1D1D

### Notes

- Cost values are engine-specific and not directly comparable across different database systems
- The component automatically handles 1-4 engines
- Best/worst highlighting only applies to numeric metrics
- Index usage shows check/cancel icons with Yes/No text

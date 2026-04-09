# ComparisonTable Example

This document shows what the ComparisonTable component looks like when rendered.

## Sample Output

When comparing three database engines, the table displays:

```
Statistical Comparison

Note: Cost values are engine-specific and not directly comparable across different
database systems. Lower costs generally indicate better performance within the same
engine.

┌─────────────────────┬─────────────────┬─────────────────┬─────────────────┐
│ Metric              │ PostgreSQL 15   │ MySQL 8.4       │ DuckDB          │
├─────────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ Total Cost          │ 100.50          │ 85.30 ✓         │ 120.00 ✗        │
│                     │ 84%             │ 71%             │ 100%            │
├─────────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ Estimated Rows      │ 1.00K ✓         │ 950 ✓✓          │ 1.10K ✗         │
├─────────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ Plan Depth          │ 3               │ 2 ✓             │ 4 ✗             │
├─────────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ Scan Operations     │ 2               │ 1 ✓             │ 1 ✓             │
├─────────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ Join Operations     │ 1               │ 0 ✓             │ 0 ✓             │
├─────────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ Sort Operations     │ 0 ✓             │ 0 ✓             │ 1 ✗             │
├─────────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ Index Usage         │ ✓ Yes           │ ✓ Yes           │ ✗ No            │
└─────────────────────┴─────────────────┴─────────────────┴─────────────────┘
```

Legend:
- ✓ (green background) = Best value for this metric
- ✗ (red background) = Worst value for this metric
- Percentage chips below Total Cost show relative cost (green < 50%, yellow 50-80%, red > 80%)
- Index Usage shows checkmark/cancel icons with Yes/No text

## Color Scheme

The table uses a dark theme consistent with CostAnalysisView:

- Background: #0F172A (dark blue-gray)
- Table cells: #1E293B (lighter blue-gray)
- Text: #F1F5F9 (light gray)
- Hover: #334155 (medium blue-gray)
- Best values: #064E3B (dark green)
- Worst values: #7F1D1D (dark red)
- Success icon: #34D399 (bright green)
- Error icon: #F87171 (bright red)

## Interactive Features

1. **Hover Effects**: Table rows highlight on hover for better readability
2. **Best/Worst Comparison**: Automatically identifies and highlights the best and worst values
3. **Percentage Indicators**: Shows cost as a percentage of the maximum for easy comparison
4. **Number Formatting**: Large numbers formatted with K/M suffixes (1,000 → 1.00K)
5. **Visual Icons**: Index usage shows clear visual indicators with icons

## Integration Example

```tsx
import { useState } from 'react';
import { Box, Button } from '@mui/material';
import { ComparisonTable } from './components/comparison';
import type { CostMetrics } from './types';

function MultiEngineComparison() {
  const [selectedPanels, setSelectedPanels] = useState<string[]>([]);

  // Gather metrics from selected output panels
  const metrics: CostMetrics[] = selectedPanels
    .map((panelId) => {
      const panel = panels.find((p) => p.id === panelId);
      return panel?.costMetrics;
    })
    .filter((m): m is CostMetrics => m !== null);

  const engineNames = selectedPanels
    .map((panelId) => {
      const panel = panels.find((p) => p.id === panelId);
      return panel?.engine;
    })
    .filter((e): e is string => e !== undefined);

  return (
    <Box>
      <Box sx={{ mb: 2 }}>
        <Button onClick={() => setSelectedPanels([...])}>
          Select Panels to Compare
        </Button>
      </Box>

      {metrics.length > 0 && (
        <ComparisonTable metrics={metrics} engineNames={engineNames} />
      )}
    </Box>
  );
}
```

## Data Flow

1. User selects 2-4 output panels with execution plans
2. Application extracts CostMetrics from each selected panel
3. ComparisonTable receives metrics array and engine names
4. Component calculates:
   - Best/worst values for each metric
   - Percentages relative to maximum
   - Operation type counts
   - Index usage flags
5. Renders table with highlighting and formatting

# Frontend Testing Guide

## Overview

This document describes the testing strategy and implementation for the RA Web frontend visualization components.

## Test Files

### Location
All tests are in `/home/gburd/ws/ra/crates/ra-web/frontend/src/__tests__/`

### Structure
```
src/__tests__/
├── setup.ts                              # Global test configuration
├── README.md                             # Test documentation
├── validate-tests.sh                     # Test structure validator
└── components/
    ├── PlanTreeView.test.tsx            # Tree visualization tests (13 tests)
    ├── CostAnalysisView.test.tsx        # Cost metrics tests (20 tests)
    └── WarningsView.test.tsx            # Warning display tests (21 tests)
```

## Test Coverage by Component

### PlanTreeView (13 tests)
Tests D3-based tree visualization rendering and interaction:
- Rendering with valid/empty/null data
- Node click callbacks
- Node highlighting
- Single node and deep nesting
- Nodes with relations
- Dynamic plan updates
- Dynamic highlight updates
- Operation color coding
- Graceful handling of missing callbacks

**Mocking Strategy**: D3 is mocked to avoid complex DOM manipulation. Tests verify D3 functions are called, not their visual output.

### CostAnalysisView (20 tests)
Tests cost metrics display and table sorting:
- Summary card rendering (total cost, rows, depth, operations)
- Operations table with all data
- Empty operation breakdown
- Sorting by operation name (asc/desc)
- Sorting by cost (default desc, toggle asc)
- Sorting by rows
- Sorting by percentage
- Sort direction toggling
- Row click callbacks
- Missing nodeId handling
- Bar chart rendering (top 10)
- Long operation name truncation
- Large number formatting
- Percentage calculation accuracy
- Single operation handling
- Color coding by operation type

**Mocking Strategy**: No mocking needed. Recharts renders SVG that can be tested directly.

### WarningsView (21 tests)
Tests warning grouping, display, and interaction:
- Success message when no warnings
- Rendering all warnings
- Severity count display (critical/warning/info)
- Grouping by warning type
- Issue count per type
- Accordion expansion/collapse
- Warning click callbacks
- Node ID display
- Severity color application
- Individual severity counts
- Multiple warnings of same type
- All warning type labels
- Separate expansion state per accordion
- Empty array handling
- Special characters in messages

**Mocking Strategy**: No mocking needed. MUI components render normally in jsdom.

## Running Tests

### Install Dependencies
```bash
cd crates/ra-web/frontend
pnpm install
```

### Run Tests
```bash
# Run all tests once
pnpm test

# Run in watch mode (auto-rerun on file changes)
pnpm test:watch

# Run with UI (browser-based test viewer)
pnpm test:ui

# Validate test file structure
bash src/__tests__/validate-tests.sh
```

## Test Philosophy

Tests follow these principles from CLAUDE.md:

1. **Test behavior, not implementation**
   - Verify what code does, not how
   - Semantic queries (getByText, getByRole) over test IDs
   - No testing internal state or private methods

2. **Test edges and errors, not just happy path**
   - Empty inputs (null, empty arrays)
   - Single items vs. multiple items
   - Large datasets
   - Missing optional props
   - Missing callbacks

3. **Mock boundaries, not logic**
   - D3 is mocked (external library with DOM manipulation)
   - Recharts is not mocked (renders predictable SVG)
   - MUI is not mocked (renders normally in jsdom)
   - No mocking of component logic

4. **Verify tests catch failures**
   - Tests are written to fail if component behavior changes
   - Each assertion tests a specific user-visible outcome

## Configuration

### vitest.config.ts
```typescript
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/__tests__/setup.ts'],
    css: true,
  },
});
```

### package.json Scripts
```json
{
  "scripts": {
    "test": "vitest run",
    "test:watch": "vitest",
    "test:ui": "vitest --ui"
  }
}
```

### Dependencies
- `vitest` - Test runner
- `@vitest/ui` - Browser UI
- `@testing-library/react` - React testing utilities
- `@testing-library/user-event` - User interaction simulation
- `@testing-library/jest-dom` - DOM matchers
- `jsdom` - DOM environment

## Adding New Tests

When adding new component tests:

1. Create test file: `src/__tests__/components/ComponentName.test.tsx`
2. Import test utilities:
   ```typescript
   import { describe, it, expect, vi, beforeEach } from 'vitest';
   import { render, screen } from '@testing-library/react';
   import userEvent from '@testing-library/user-event';
   ```
3. Create mock data factories
4. Write tests in order:
   - Basic rendering
   - Null/empty data
   - User interactions
   - Callbacks
   - Edge cases
5. Update validate-tests.sh to include new file
6. Update this document

## Common Patterns

### Mock Data Factory
```typescript
const createMockNode = (id: string, operation: string): PlanNode => ({
  id,
  operation,
  relation: null,
  cost: { startup: 0, total: 100 },
  rows: 1000,
  children: [],
  metadata: {},
});
```

### User Interaction
```typescript
const user = userEvent.setup();
await user.click(element);
```

### Callback Testing
```typescript
const onNodeClick = vi.fn();
render(<Component onNodeClick={onNodeClick} />);
await user.click(element);
expect(onNodeClick).toHaveBeenCalledWith('nodeId');
```

### Finding Elements
```typescript
// Prefer semantic queries
screen.getByRole('button', { name: 'Submit' });
screen.getByText('Warning message');
screen.getByLabelText('Username');

// Query all rows, then inspect specific cells
const rows = screen.getAllByRole('row');
const cells = within(rows[1]).getAllByRole('cell');
```

## Troubleshooting

### Tests fail with "ReferenceError: ... is not defined"
Add missing globals to `src/__tests__/setup.ts`:
```typescript
global.ResizeObserver = vi.fn().mockImplementation(() => ({
  observe: vi.fn(),
  unobserve: vi.fn(),
  disconnect: vi.fn(),
}));
```

### Tests fail with "Cannot find module"
Check import paths are relative from test file:
```typescript
import { Component } from '../../components/Component';
import type { Type } from '../../types';
```

### D3 tests fail with DOM errors
Ensure D3 is properly mocked in test file (see PlanTreeView.test.tsx).

### MUI components not rendering
Install `@testing-library/jest-dom` and import in setup.ts.

## Next Steps

Future testing work:
1. Add parser unit tests (`src/utils/__tests__/`)
2. Add E2E tests with Playwright
3. Add visual regression tests
4. Measure code coverage with `vitest --coverage`

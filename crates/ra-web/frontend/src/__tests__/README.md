# Frontend Unit Tests

This directory contains unit tests for the RA Web frontend components.

## Test Structure

- `setup.ts` - Global test setup configuration
- `components/` - Component tests
  - `PlanTreeView.test.tsx` - D3 tree visualization tests
  - `CostAnalysisView.test.tsx` - Cost metrics display tests
  - `WarningsView.test.tsx` - Warning grouping and display tests

## Running Tests

```bash
# Run all tests once
pnpm test

# Run tests in watch mode
pnpm test:watch

# Run tests with UI
pnpm test:ui
```

## Test Coverage

Each component test covers:
- Rendering with valid props
- Handling null/empty data
- User interactions (clicks, sorting, expanding/collapsing)
- Callback function behavior
- Edge cases (empty arrays, single items, large datasets)

## Mocking Strategy

- **D3**: Mocked to avoid DOM manipulation complexity
- **React Flow**: Not needed in current tests (Flow View uses @xyflow/react)
- **Recharts**: Renders without mocking (uses SVG)

## Test Philosophy

Tests focus on behavior, not implementation:
- Verify what the component does, not how it does it
- Test user-visible outcomes
- Mock only external dependencies (D3 DOM manipulation)
- Use semantic queries (getByRole, getByText) over test IDs

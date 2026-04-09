# E2E Tests

End-to-end tests for the RA SQL Query Optimizer web interface using Playwright.

## Test Coverage

### Full Workflow Tests (`full-workflow.spec.ts`)

1. **Complete Visualization Workflow**
   - Page loading and core UI elements
   - Schema selection and sample query loading
   - Query execution
   - Tab switching between all 5 visualization types
   - Verification of each tab rendering

2. **Multi-Panel Comparison**
   - Adding panels
   - Setting different engines per panel
   - Executing queries across panels
   - Verifying results in all panels

3. **URL Sharing**
   - Generating share URLs
   - Copying URLs from share dialog
   - Restoring state from URLs
   - Opening URLs in new sessions

4. **Search Functionality**
   - Opening search interface
   - Searching for terms in plans
   - Navigating between matches
   - Search highlighting across tabs

5. **Error Handling**
   - Invalid SQL errors
   - Network timeout handling

6. **Performance and Responsiveness**
   - Large query plan handling
   - Visualization rendering times

7. **Accessibility**
   - Keyboard navigation
   - ARIA labels and roles

### Edge Cases Tests (`edge-cases.spec.ts`)

1. **State Management**
   - Query preservation across panel additions
   - Independent engine selections
   - Tab selection persistence

2. **URL Encoding Edge Cases**
   - Special characters in SQL
   - Multi-line queries
   - Very long URLs
   - Multi-panel state restoration

3. **Search Edge Cases**
   - Case-insensitive search
   - Cross-tab search persistence
   - No matches handling
   - Search clearing on re-execution

4. **Panel Management**
   - Maximum panel limit
   - Independent query execution
   - State maintenance across tabs

5. **Visualization Rendering**
   - Empty plan results
   - Deeply nested plans
   - Multiple join types

6. **Copy Functionality**
   - Plan text copying
   - Share URL copying

7. **Browser Compatibility**
   - Back button handling
   - Page refresh

8. **Concurrent Operations**
   - Rapid tab switching
   - Multiple search operations

9. **Memory and Performance**
   - Repeated executions
   - Visualization cleanup

## Running Tests

### Prerequisites

Install dependencies:

```bash
cd crates/ra-web/frontend
npm install
npx playwright install
```

### Run All Tests

```bash
npm run test:e2e
```

### Run Specific Test File

```bash
npx playwright test e2e/full-workflow.spec.ts
```

### Run in UI Mode

```bash
npx playwright test --ui
```

### Debug Mode

```bash
npx playwright test --debug
```

### Run Specific Browser

```bash
npx playwright test --project=chromium
```

## Test Structure

### Fixtures (`fixtures.ts`)

Helper functions and constants for common test operations:

- `typeInEditor()` - Type SQL in Monaco editor
- `executeQuery()` - Click execute button
- `waitForQueryCompletion()` - Wait for query to finish
- `switchToTab()` - Switch between visualization tabs
- `selectSchema()` - Select schema and load sample query
- `addPanel()` - Add comparison panel
- `openShareDialog()` - Open and get share URL
- `searchInPlan()` - Search for terms in plan
- Sample queries for testing
- Engine and schema configurations

## Configuration

The Playwright configuration is in `/home/gburd/ws/ra/crates/ra-web/frontend/playwright.config.ts`.

Key settings:

- Base URL: `http://localhost:5173`
- Test directory: `/home/gburd/ws/ra/e2e`
- Retries: 2 in CI, 0 locally
- Trace: On first retry
- Screenshot: On failure
- Web server: Auto-start with `npm run dev`

## Writing New Tests

Use the fixtures for common operations:

```typescript
import { test, expect } from '@playwright/test';
import { typeInEditor, executeQuery, waitForQueryCompletion, SAMPLE_QUERIES } from './fixtures';

test('my test', async ({ page }) => {
  await page.goto('http://localhost:5173');

  await typeInEditor(page, SAMPLE_QUERIES.simple);
  await executeQuery(page);
  await waitForQueryCompletion(page);

  await expect(page.locator('text=Raw Plan')).toBeVisible();
});
```

## Test Data

Sample queries are defined in `fixtures.ts`:

- `simple` - Basic SELECT with WHERE
- `join` - JOIN query
- `aggregation` - GROUP BY query
- `subquery` - Nested subquery
- `complex` - Multi-join with filters

## CI Integration

Tests are configured to run in CI with:

- `forbidOnly: true` - Fail if `.only` is present
- `retries: 2` - Retry failed tests twice
- `workers: 1` - Run serially in CI

## Debugging

### View Test Report

```bash
npx playwright show-report
```

### Generate Trace

Traces are automatically generated on first retry. View them:

```bash
npx playwright show-trace trace.zip
```

### Screenshot on Failure

Screenshots are automatically saved to `test-results/` on failure.

### Inspector

```bash
npx playwright test --debug
```

## Best Practices

1. Use fixtures for common operations
2. Use `waitForQueryCompletion()` after query execution
3. Add timeouts for async operations
4. Use `.or()` for elements that may vary
5. Handle race conditions with proper waits
6. Test error states, not just happy paths
7. Verify accessibility features
8. Test different browsers in CI

## Known Issues

- Monaco editor content extraction may vary slightly between browsers
- Some visualizations require longer wait times for complex plans
- Search functionality may need timing adjustments for large plans

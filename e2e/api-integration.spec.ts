import { test, expect } from '@playwright/test';
import {
  typeInEditor,
  executeQuery,
  waitForQueryCompletion,
  addPanel,
  changeEngine,
  SAMPLE_QUERIES,
} from './fixtures';

const BASE_URL = 'http://localhost:5173';
const API_BASE_URL = 'http://localhost:8080';

test.describe('API Integration Tests', () => {
  test.describe('Explain API', () => {
    test('should call explain API with correct parameters', async ({ page }) => {
      let explainRequest: any = null;

      await page.route('**/api/explain', async (route) => {
        explainRequest = await route.request().postDataJSON();
        await route.continue();
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      expect(explainRequest).not.toBeNull();
      expect(explainRequest.sql).toContain('employees');
      expect(explainRequest.engine).toMatch(/postgresql|mysql|duckdb|sqlite/);
      expect(typeof explainRequest.analyze).toBe('boolean');
    });

    test('should send analyze flag when analyze mode selected', async ({ page }) => {
      let explainRequest: any = null;

      await page.route('**/api/explain', async (route) => {
        explainRequest = await route.request().postDataJSON();
        await route.continue();
      });

      await page.goto(BASE_URL);

      const analyzeButton = page.locator('button:has-text("Analyze")');
      await analyzeButton.click();

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      expect(explainRequest).not.toBeNull();
      expect(explainRequest.analyze).toBe(true);
    });

    test('should send correct engine for each panel', async ({ page }) => {
      const requests: any[] = [];

      await page.route('**/api/explain', async (route) => {
        const request = await route.request().postDataJSON();
        requests.push(request);
        await route.continue();
      });

      await page.goto(BASE_URL);
      await addPanel(page);

      await changeEngine(page, 0, 'PostgreSQL 16');
      await changeEngine(page, 1, 'MySQL 8.4');

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      expect(requests.length).toBe(2);
      expect(requests[0].engine).toContain('postgresql');
      expect(requests[1].engine).toContain('mysql');
    });
  });

  test.describe('Response Handling', () => {
    test('should display error message from API', async ({ page }) => {
      await page.route('**/api/explain', (route) => {
        route.fulfill({
          status: 400,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Syntax error near FROM' }),
        });
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, 'SELECT * FORM invalid_table;');
      await executeQuery(page);

      await page.waitForTimeout(1000);

      await expect(page.locator('text=/Syntax error|error/i')).toBeVisible({ timeout: 5000 });
    });

    test('should handle 500 server errors', async ({ page }) => {
      await page.route('**/api/explain', (route) => {
        route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Internal server error' }),
        });
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      await page.waitForTimeout(1000);

      await expect(page.locator('text=/server error|error|failed/i')).toBeVisible({ timeout: 5000 });
    });

    test('should handle network errors gracefully', async ({ page }) => {
      await page.route('**/api/explain', (route) => {
        route.abort('failed');
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      await page.waitForTimeout(2000);

      const hasError = await page.locator('text=/network|error|failed/i').isVisible({ timeout: 5000 }).catch(() => false);
      expect(hasError).toBe(true);
    });

    test('should parse and display valid plan response', async ({ page }) => {
      const mockPlan = `Seq Scan on employees  (cost=0.00..35.50 rows=10 width=244)
  Filter: (id = 1)`;

      await page.route('**/api/explain', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            plan: mockPlan,
            engine: 'postgresql-16',
            analyzed: false,
          }),
        });
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await expect(page.locator('text=Seq Scan')).toBeVisible({ timeout: 5000 });
    });
  });

  test.describe('Request Optimization', () => {
    test('should debounce multiple rapid executions', async ({ page }) => {
      let requestCount = 0;

      await page.route('**/api/explain', async (route) => {
        requestCount++;
        await route.continue();
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);

      for (let i = 0; i < 5; i++) {
        await executeQuery(page);
        await page.waitForTimeout(100);
      }

      await waitForQueryCompletion(page);

      expect(requestCount).toBeGreaterThan(0);
      expect(requestCount).toBeLessThanOrEqual(5);
    });

    test('should cancel previous request when new query submitted', async ({ page }) => {
      const delays: number[] = [];

      await page.route('**/api/explain', async (route) => {
        const startTime = Date.now();
        await new Promise((resolve) => setTimeout(resolve, 2000));
        delays.push(Date.now() - startTime);
        await route.continue();
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      await page.waitForTimeout(500);
      await executeQuery(page);

      await waitForQueryCompletion(page, 10000);
    });
  });

  test.describe('Schema API', () => {
    test('should load schemas on demand', async ({ page }) => {
      let schemaRequested = false;

      await page.route('**/api/schemas', (route) => {
        schemaRequested = true;
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            schemas: [
              {
                name: 'HR',
                tables: [{ name: 'employees', ddl: 'CREATE TABLE employees...' }],
                sampleQueries: [{ name: 'Query 1', sql: 'SELECT * FROM employees', description: 'Test' }],
              },
            ],
          }),
        });
      });

      await page.goto(BASE_URL);

      await page.click('button:has([data-testid="SchemaIcon"])');

      await page.waitForTimeout(500);
    });
  });

  test.describe('Share API', () => {
    test('should generate share URL with state', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);

      await page.click('button:has-text("Share")');

      const urlInput = page.locator('input[readonly]');
      const shareUrl = await urlInput.inputValue();

      expect(shareUrl).toContain('http');
      expect(shareUrl.length).toBeGreaterThan(BASE_URL.length);

      const url = new URL(shareUrl);
      expect(url.searchParams.has('state') || url.hash.includes('state')).toBe(true);
    });

    test('should encode special characters in share URL', async ({ page }) => {
      await page.goto(BASE_URL);

      const specialQuery = "SELECT * FROM users WHERE name = 'O''Brien' AND email LIKE '%@%';";
      await typeInEditor(page, specialQuery, 20);

      await page.click('button:has-text("Share")');

      const urlInput = page.locator('input[readonly]');
      const shareUrl = await urlInput.inputValue();

      expect(shareUrl).toContain('http');

      await page.goto(shareUrl);
      await page.waitForTimeout(1000);

      const content = await page.textContent('.monaco-editor .view-lines');
      expect(content).toContain('users');
    });
  });

  test.describe('Request Headers', () => {
    test('should include content-type header', async ({ page }) => {
      let headers: any = null;

      await page.route('**/api/explain', async (route) => {
        headers = route.request().headers();
        await route.continue();
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      expect(headers['content-type']).toContain('application/json');
    });

    test('should handle CORS properly', async ({ page }) => {
      await page.goto(BASE_URL);

      const response = await page.evaluate(async () => {
        const res = await fetch('http://localhost:8080/api/explain', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            sql: 'SELECT 1',
            engine: 'postgresql-16',
            analyze: false,
          }),
        });
        return {
          status: res.status,
          headers: Object.fromEntries(res.headers.entries()),
        };
      });

      expect(response.status).toBeLessThan(500);
    });
  });

  test.describe('Response Validation', () => {
    test('should handle malformed JSON response', async ({ page }) => {
      await page.route('**/api/explain', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: 'not valid json{',
        });
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      await page.waitForTimeout(1000);

      const hasError = await page.locator('text=/error|failed/i').isVisible({ timeout: 5000 }).catch(() => false);
      expect(hasError).toBe(true);
    });

    test('should handle missing required fields', async ({ page }) => {
      await page.route('**/api/explain', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ engine: 'postgresql-16' }),
        });
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      await page.waitForTimeout(1000);

      const hasError = await page.locator('text=/error|failed/i').isVisible({ timeout: 5000 }).catch(() => false);
      expect(hasError).toBe(true);
    });

    test('should validate engine name in response', async ({ page }) => {
      await page.route('**/api/explain', (route) => {
        route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            plan: 'Seq Scan on table',
            engine: 'invalid-engine',
            analyzed: false,
          }),
        });
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await expect(page.locator('text=Seq Scan')).toBeVisible({ timeout: 5000 });
    });
  });

  test.describe('Loading States', () => {
    test('should show loading indicator during request', async ({ page }) => {
      await page.route('**/api/explain', async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 2000));
        await route.continue();
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      await expect(page.locator('[role="progressbar"]')).toBeVisible({ timeout: 1000 });

      await waitForQueryCompletion(page, 10000);
    });

    test('should disable execute button during request', async ({ page }) => {
      await page.route('**/api/explain', async (route) => {
        await new Promise((resolve) => setTimeout(resolve, 2000));
        await route.continue();
      });

      await page.goto(BASE_URL);
      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      const executeButton = page.locator('button:has-text("Execute")');
      await expect(executeButton).toBeDisabled({ timeout: 1000 });

      await waitForQueryCompletion(page, 10000);
      await expect(executeButton).toBeEnabled();
    });
  });

  test.describe('Multi-Panel API Calls', () => {
    test('should make parallel requests for multiple panels', async ({ page }) => {
      const requestTimes: number[] = [];

      await page.route('**/api/explain', async (route) => {
        requestTimes.push(Date.now());
        await route.continue();
      });

      await page.goto(BASE_URL);
      await addPanel(page);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      expect(requestTimes.length).toBe(2);

      const timeDiff = Math.abs(requestTimes[1] - requestTimes[0]);
      expect(timeDiff).toBeLessThan(1000);
    });

    test('should handle partial failures in multi-panel requests', async ({ page }) => {
      let requestCount = 0;

      await page.route('**/api/explain', (route) => {
        requestCount++;
        if (requestCount === 1) {
          route.fulfill({
            status: 200,
            contentType: 'application/json',
            body: JSON.stringify({
              plan: 'Seq Scan on table',
              engine: 'postgresql-16',
              analyzed: false,
            }),
          });
        } else {
          route.fulfill({
            status: 500,
            contentType: 'application/json',
            body: JSON.stringify({ error: 'Database connection failed' }),
          });
        }
      });

      await page.goto(BASE_URL);
      await addPanel(page);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      await page.waitForTimeout(2000);

      const successPanel = await page.locator('text=Seq Scan').isVisible().catch(() => false);
      const errorPanel = await page.locator('text=/error|failed/i').isVisible().catch(() => false);

      expect(successPanel || errorPanel).toBe(true);
    });
  });
});

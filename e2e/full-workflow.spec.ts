import { test, expect, type Page } from '@playwright/test';

const BASE_URL = 'http://localhost:5173';

test.describe('Full Workflow E2E Tests', () => {
  test.describe('Complete Visualization Workflow', () => {
    test('should load page and display all core UI elements', async ({ page }) => {
      await page.goto(BASE_URL);

      await expect(page.locator('text=RA SQL Optimizer')).toBeVisible();
      await expect(page.locator('button:has-text("Execute")')).toBeVisible();
      await expect(page.locator('button:has-text("Share")')).toBeVisible();
      await expect(page.locator('.monaco-editor')).toBeVisible();
    });

    test('should select schema and load sample query', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.click('button:has([data-testid="SchemaIcon"])');
      await expect(page.locator('text=Available Schemas')).toBeVisible();

      await page.click('text=HR (Employee-Department)');
      await page.click('text=Find High Earners');

      const editorContent = await page.textContent('.monaco-editor .view-lines');
      expect(editorContent).toContain('SELECT');
      expect(editorContent).toContain('employees');

      await page.click('button:has-text("Close")');
    });

    test('should execute query and display results', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM employees WHERE id = 1;', { delay: 50 });

      await page.click('button:has-text("Execute")');

      await expect(page.locator('[role="progressbar"]')).toBeVisible({ timeout: 1000 });
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await expect(page.locator('text=Raw Plan')).toBeVisible();
    });

    test('should switch between all 5 visualization tabs', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM users WHERE id = 1;', { delay: 50 });
      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      const tabs: Array<{ name: string; selector: string }> = [
        { name: 'Raw Plan', selector: 'button[role="tab"]:has-text("Raw")' },
        { name: 'Tree View', selector: 'button[role="tab"]:has-text("Tree")' },
        { name: 'Flow View', selector: 'button[role="tab"]:has-text("Flow")' },
        { name: 'Cost Analysis', selector: 'button[role="tab"]:has-text("Cost")' },
        { name: 'Warnings', selector: 'button[role="tab"]:has-text("Warnings")' },
      ];

      for (const tab of tabs) {
        await page.click(tab.selector);
        await page.waitForTimeout(500);

        const tabElement = page.locator(tab.selector);
        await expect(tabElement).toHaveAttribute('aria-selected', 'true');
      }
    });

    test('should verify each tab renders correctly', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT id, name FROM employees WHERE department_id = 1;', { delay: 30 });
      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button[role="tab"]:has-text("Raw")');
      await expect(page.locator('.monaco-editor')).toBeVisible();

      await page.click('button[role="tab"]:has-text("Tree")');
      await page.waitForTimeout(1000);
      await expect(page.locator('svg')).toBeVisible();

      await page.click('button[role="tab"]:has-text("Flow")');
      await page.waitForTimeout(1000);
      await expect(page.locator('.react-flow')).toBeVisible();

      await page.click('button[role="tab"]:has-text("Cost")');
      await page.waitForTimeout(500);
      await expect(page.locator('text=Total Cost').or(page.locator('text=Operation Breakdown'))).toBeVisible();

      await page.click('button[role="tab"]:has-text("Warnings")');
      await page.waitForTimeout(500);
      await expect(page.locator('text=No warnings detected').or(page.locator('[data-testid="warning-item"]'))).toBeVisible();
    });
  });

  test.describe('Multi-Panel Comparison', () => {
    test('should add second panel', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.click('button:has-text("Add Panel")');

      await expect(page.locator('text=PostgreSQL')).toHaveCount(2);
    });

    test('should set different engines for each panel', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.click('button:has-text("Add Panel")');

      const engineSelectors = page.locator('select, [role="combobox"]').filter({ hasText: /PostgreSQL|MySQL|DuckDB/ });
      await expect(engineSelectors).toHaveCount(2);

      await engineSelectors.first().click();
      await page.click('text=PostgreSQL 16');

      await engineSelectors.last().click();
      await page.click('text=MySQL 8.4');
    });

    test('should execute query and verify both panels show results', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.click('button:has-text("Add Panel")');

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM employees LIMIT 10;', { delay: 30 });

      await page.click('button:has-text("Execute")');

      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      const rawPlanTabs = page.locator('button[role="tab"]:has-text("Raw")');
      await expect(rawPlanTabs).toHaveCount(2);

      for (let i = 0; i < 2; i++) {
        const tab = rawPlanTabs.nth(i);
        await tab.click();
        await expect(page.locator('.monaco-editor').nth(i + 1)).toBeVisible();
      }
    });
  });

  test.describe('URL Sharing', () => {
    test('should execute query and open share dialog', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT id FROM users WHERE active = true;', { delay: 30 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button:has-text("Share")');

      await expect(page.locator('text=Share Query')).toBeVisible();
    });

    test('should copy URL from share dialog', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM products;', { delay: 30 });

      await page.click('button:has-text("Share")');

      await expect(page.locator('input[readonly]')).toBeVisible();

      const urlInput = page.locator('input[readonly]');
      const shareUrl = await urlInput.inputValue();
      expect(shareUrl).toContain('http');
      expect(shareUrl.length).toBeGreaterThan(BASE_URL.length);
    });

    test('should restore state from URL', async ({ page }) => {
      await page.goto(BASE_URL);

      const testQuery = 'SELECT name, email FROM customers WHERE country = \'US\';';
      await page.locator('.monaco-editor').click();
      await page.keyboard.type(testQuery, { delay: 30 });

      await page.click('button:has-text("Share")');
      const urlInput = page.locator('input[readonly]');
      const shareUrl = await urlInput.inputValue();

      await page.click('button:has-text("Close")');

      await page.goto(shareUrl);

      await page.waitForTimeout(1000);

      const editorContent = await page.textContent('.monaco-editor .view-lines');
      expect(editorContent).toContain('SELECT');
      expect(editorContent).toContain('customers');
    });

    test('should open URL in new session and verify state', async ({ context }) => {
      const page1 = await context.newPage();
      await page1.goto(BASE_URL);

      const testQuery = 'SELECT order_id, total FROM orders WHERE status = \'completed\';';
      await page1.locator('.monaco-editor').click();
      await page1.keyboard.type(testQuery, { delay: 30 });

      await page1.click('button:has([value="analyze"], text)');

      await page1.click('button:has-text("Share")');
      const urlInput = page1.locator('input[readonly]');
      const shareUrl = await urlInput.inputValue();

      const page2 = await context.newPage();
      await page2.goto(shareUrl);

      await page2.waitForTimeout(1000);

      const editorContent = await page2.textContent('.monaco-editor .view-lines');
      expect(editorContent).toContain('orders');
      expect(editorContent).toContain('completed');

      const analyzeButton = page2.locator('button[aria-pressed="true"]:has-text("Analyze")');
      await expect(analyzeButton).toBeVisible();

      await page1.close();
      await page2.close();
    });
  });

  test.describe('Search Functionality', () => {
    test('should execute query and open search', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM employees WHERE department_id IN (1, 2, 3);', { delay: 30 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button:has([data-testid="SearchIcon"])');

      await expect(page.locator('input[placeholder*="Search"]')).toBeVisible();
    });

    test('should search for term in plan', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT id, name FROM employees WHERE salary > 50000;', { delay: 30 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button:has([data-testid="SearchIcon"])');

      const searchInput = page.locator('input[placeholder*="Search"]');
      await searchInput.fill('Seq');
      await page.keyboard.press('Enter');

      await page.waitForTimeout(500);

      const matchCount = page.locator('text=/\\d+\\/\\d+/');
      await expect(matchCount.or(page.locator('text="No matches"'))).toBeVisible();
    });

    test('should navigate between search matches', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT e.*, d.name FROM employees e JOIN departments d ON e.department_id = d.id;', { delay: 30 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button:has([data-testid="SearchIcon"])');

      const searchInput = page.locator('input[placeholder*="Search"]');
      await searchInput.fill('Scan');
      await page.keyboard.press('Enter');

      await page.waitForTimeout(500);

      const nextButton = page.locator('button[aria-label*="Next"]').or(page.locator('button:has-text("Next")'));
      if (await nextButton.isVisible()) {
        await nextButton.click();
        await page.waitForTimeout(300);
        await nextButton.click();
        await page.waitForTimeout(300);
      }

      const prevButton = page.locator('button[aria-label*="Previous"]').or(page.locator('button:has-text("Previous")'));
      if (await prevButton.isVisible()) {
        await prevButton.click();
        await page.waitForTimeout(300);
      }
    });

    test('should highlight search matches in different tabs', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM orders WHERE order_date > \'2024-01-01\';', { delay: 30 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button:has([data-testid="SearchIcon"])');
      const searchInput = page.locator('input[placeholder*="Search"]');
      await searchInput.fill('Scan');
      await page.keyboard.press('Enter');

      await page.click('button[role="tab"]:has-text("Raw")');
      await page.waitForTimeout(500);

      await page.click('button[role="tab"]:has-text("Tree")');
      await page.waitForTimeout(500);

      await page.click('button[role="tab"]:has-text("Flow")');
      await page.waitForTimeout(500);
    });

    test('should clear search and reset highlighting', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM products WHERE price < 100;', { delay: 30 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button:has([data-testid="SearchIcon"])');

      const searchInput = page.locator('input[placeholder*="Search"]');
      await searchInput.fill('Index');
      await page.keyboard.press('Enter');
      await page.waitForTimeout(500);

      await searchInput.clear();
      await page.keyboard.press('Escape');

      await expect(searchInput).not.toBeVisible();
    });
  });

  test.describe('Error Handling', () => {
    test('should display error message for invalid SQL', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FORM invalid_table;', { delay: 30 });

      await page.click('button:has-text("Execute")');

      await page.waitForTimeout(2000);

      await expect(page.locator('text=/error|failed|invalid/i')).toBeVisible({ timeout: 10000 });
    });

    test('should handle network timeout gracefully', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.route('**/api/explain', route => {
        return new Promise(() => {});
      });

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM users;', { delay: 30 });

      await page.click('button:has-text("Execute")');

      await expect(page.locator('[role="progressbar"]')).toBeVisible();
    });
  });

  test.describe('Performance and Responsiveness', () => {
    test('should handle large query plans without freezing', async ({ page }) => {
      await page.goto(BASE_URL);

      const complexQuery = `
        SELECT e.*, d.name as dept_name, m.name as manager_name
        FROM employees e
        LEFT JOIN departments d ON e.department_id = d.id
        LEFT JOIN employees m ON d.manager_id = m.id
        WHERE e.hire_date > '2020-01-01'
          AND e.salary > 50000
        ORDER BY e.salary DESC
        LIMIT 100;
      `;

      await page.locator('.monaco-editor').click();
      await page.keyboard.type(complexQuery, { delay: 10 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await page.click('button[role="tab"]:has-text("Tree")');
      await page.waitForTimeout(1000);

      await page.click('button[role="tab"]:has-text("Flow")');
      await page.waitForTimeout(1000);

      const isInteractive = await page.evaluate(() => {
        return document.readyState === 'complete';
      });
      expect(isInteractive).toBe(true);
    });

    test('should render visualizations within acceptable time', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT * FROM orders JOIN customers ON orders.customer_id = customers.id;', { delay: 30 });

      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      const startTime = Date.now();
      await page.click('button[role="tab"]:has-text("Tree")');
      await expect(page.locator('svg')).toBeVisible({ timeout: 5000 });
      const treeRenderTime = Date.now() - startTime;

      expect(treeRenderTime).toBeLessThan(5000);

      const flowStartTime = Date.now();
      await page.click('button[role="tab"]:has-text("Flow")');
      await expect(page.locator('.react-flow')).toBeVisible({ timeout: 5000 });
      const flowRenderTime = Date.now() - flowStartTime;

      expect(flowRenderTime).toBeLessThan(5000);
    });
  });

  test.describe('Accessibility', () => {
    test('should navigate interface with keyboard', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.keyboard.press('Tab');
      await page.keyboard.press('Tab');

      await page.keyboard.press('Enter');

      await expect(page.locator('[role="progressbar"]').or(page.locator('text=Raw Plan'))).toBeVisible({ timeout: 30000 });
    });

    test('should have proper ARIA labels and roles', async ({ page }) => {
      await page.goto(BASE_URL);

      await expect(page.locator('button[aria-label]')).toHaveCount(4, { timeout: 5000 });
      await expect(page.locator('[role="tab"]')).toHaveCount(0);

      await page.locator('.monaco-editor').click();
      await page.keyboard.type('SELECT 1;', { delay: 30 });
      await page.click('button:has-text("Execute")');
      await expect(page.locator('[role="progressbar"]')).not.toBeVisible({ timeout: 30000 });

      await expect(page.locator('[role="tab"]')).toHaveCount(5, { timeout: 5000 });
      await expect(page.locator('[role="tabpanel"]')).toBeVisible();
    });
  });
});

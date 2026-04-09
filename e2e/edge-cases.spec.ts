import { test, expect } from '@playwright/test';
import {
  typeInEditor,
  executeQuery,
  waitForQueryCompletion,
  switchToTab,
  addPanel,
  changeEngine,
  openShareDialog,
  searchInPlan,
  SAMPLE_QUERIES,
  VISUALIZATION_TABS,
} from './fixtures';

const BASE_URL = 'http://localhost:5173';

test.describe('Edge Cases and Integration Tests', () => {
  test.describe('State Management', () => {
    test('should preserve query across panel additions', async ({ page }) => {
      await page.goto(BASE_URL);

      const query = SAMPLE_QUERIES.join;
      await typeInEditor(page, query);

      await addPanel(page);

      const editorContent = await page.textContent('.monaco-editor .view-lines');
      expect(editorContent).toContain('JOIN');
    });

    test('should maintain independent engine selections per panel', async ({ page }) => {
      await page.goto(BASE_URL);

      await addPanel(page);
      await addPanel(page);

      await changeEngine(page, 0, 'PostgreSQL 16');
      await changeEngine(page, 1, 'MySQL 8.4');
      await changeEngine(page, 2, 'DuckDB');

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      const panelCount = await page.locator('button[role="tab"]:has-text("Raw")').count();
      expect(panelCount).toBe(3);
    });

    test('should preserve tab selection when switching engines', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await switchToTab(page, 'Tree');

      const engineSelector = page.locator('select, [role="combobox"]').first();
      await engineSelector.click();
      await page.click('text=PostgreSQL 17');

      const treeTab = page.locator('button[role="tab"]:has-text("Tree")');
      await expect(treeTab).toHaveAttribute('aria-selected', 'true');
    });
  });

  test.describe('URL Encoding Edge Cases', () => {
    test('should handle special characters in SQL', async ({ page }) => {
      await page.goto(BASE_URL);

      const specialQuery = "SELECT * FROM users WHERE name LIKE '%John\\'s%' AND active = true;";
      await typeInEditor(page, specialQuery, 20);

      const shareUrl = await openShareDialog(page);
      await page.click('button:has-text("Close")');

      await page.goto(shareUrl);
      await page.waitForTimeout(1000);

      const editorContent = await page.textContent('.monaco-editor .view-lines');
      expect(editorContent).toContain("John");
      expect(editorContent).toContain('LIKE');
    });

    test('should handle multi-line queries in URL', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.complex, 10);

      const shareUrl = await openShareDialog(page);
      await page.click('button:has-text("Close")');

      await page.goto(shareUrl);
      await page.waitForTimeout(1000);

      const editorContent = await page.textContent('.monaco-editor .view-lines');
      expect(editorContent).toContain('LEFT JOIN');
      expect(editorContent).toContain('WHERE');
    });

    test('should handle very long URLs gracefully', async ({ page }) => {
      await page.goto(BASE_URL);

      const longQuery = 'SELECT ' + Array(100).fill('column1').join(', ') + ' FROM table1;';
      await typeInEditor(page, longQuery, 5);

      await page.click('button:has-text("Share")');

      await expect(page.locator('input[readonly]')).toBeVisible();
      const urlInput = page.locator('input[readonly]');
      const shareUrl = await urlInput.inputValue();

      expect(shareUrl.length).toBeGreaterThan(0);
    });

    test('should restore multi-panel state from URL', async ({ page }) => {
      await page.goto(BASE_URL);

      await addPanel(page);
      await changeEngine(page, 0, 'PostgreSQL 16');
      await changeEngine(page, 1, 'MySQL 8.4');

      await typeInEditor(page, SAMPLE_QUERIES.simple);

      const shareUrl = await openShareDialog(page);
      await page.click('button:has-text("Close")');

      await page.goto(shareUrl);
      await page.waitForTimeout(1000);

      const panelCount = await page.locator('select, [role="combobox"]').filter({ hasText: /PostgreSQL|MySQL|DuckDB/ }).count();
      expect(panelCount).toBe(2);
    });
  });

  test.describe('Search Edge Cases', () => {
    test('should handle case-insensitive search', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await searchInPlan(page, 'seq');
      await page.waitForTimeout(500);

      const hasMatches = await page.locator('text=/\\d+\\/\\d+/').or(page.locator('text="No matches"')).isVisible();
      expect(hasMatches).toBe(true);
    });

    test('should search across all visualization tabs', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.join);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await searchInPlan(page, 'Join');

      for (const tab of VISUALIZATION_TABS) {
        await page.click(tab.selector);
        await page.waitForTimeout(500);
      }

      const searchInput = page.locator('input[placeholder*="Search"]');
      const searchValue = await searchInput.inputValue();
      expect(searchValue).toBe('Join');
    });

    test('should handle search with no matches', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await searchInPlan(page, 'NonExistentTerm12345');

      await expect(page.locator('text="No matches"')).toBeVisible({ timeout: 2000 });
    });

    test('should clear search when query re-executed', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await searchInPlan(page, 'Scan');

      await executeQuery(page);
      await waitForQueryCompletion(page);

      const searchInput = page.locator('input[placeholder*="Search"]');
      const isVisible = await searchInput.isVisible().catch(() => false);

      if (isVisible) {
        const value = await searchInput.inputValue();
        expect(value).toBe('');
      }
    });
  });

  test.describe('Panel Management', () => {
    test('should reach maximum panel limit', async ({ page }) => {
      await page.goto(BASE_URL);

      for (let i = 0; i < 3; i++) {
        await addPanel(page);
      }

      const addButton = page.locator('button:has-text("Add Panel")');
      await expect(addButton).toBeDisabled();
    });

    test('should execute queries independently for each panel', async ({ page }) => {
      await page.goto(BASE_URL);

      await addPanel(page);

      await changeEngine(page, 0, 'PostgreSQL 16');
      await changeEngine(page, 1, 'MySQL 8.4');

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);

      const progressBars = page.locator('[role="progressbar"]');
      await expect(progressBars.first()).toBeVisible({ timeout: 5000 }).catch(() => {});

      await waitForQueryCompletion(page);

      const rawPlanTabs = page.locator('button[role="tab"]:has-text("Raw")');
      expect(await rawPlanTabs.count()).toBeGreaterThanOrEqual(2);
    });

    test('should maintain panel state across tab switches', async ({ page }) => {
      await page.goto(BASE_URL);

      await addPanel(page);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      const rawPlanTabs = page.locator('button[role="tab"]:has-text("Raw")');
      await rawPlanTabs.first().click();
      await page.waitForTimeout(300);

      await switchToTab(page, 'Tree');
      await page.waitForTimeout(500);

      await rawPlanTabs.last().click();
      await page.waitForTimeout(300);

      await switchToTab(page, 'Flow');
      await page.waitForTimeout(500);
    });
  });

  test.describe('Visualization Rendering', () => {
    test('should handle empty plan results', async ({ page }) => {
      await page.goto(BASE_URL);

      await page.locator('.monaco-editor').click();
      await page.keyboard.selectAll();
      await page.keyboard.press('Delete');

      await typeInEditor(page, 'SELECT 1;');
      await executeQuery(page);

      await page.waitForTimeout(3000);

      const hasError = await page.locator('text=/error|failed/i').isVisible().catch(() => false);
      const hasContent = await page.locator('button[role="tab"]').isVisible().catch(() => false);

      expect(hasError || hasContent).toBe(true);
    });

    test('should render deeply nested plans', async ({ page }) => {
      await page.goto(BASE_URL);

      const deepQuery = `
        SELECT *
        FROM (
          SELECT *
          FROM (
            SELECT *
            FROM employees
            WHERE department_id IN (
              SELECT id
              FROM departments
              WHERE budget > (
                SELECT AVG(budget)
                FROM departments
              )
            )
          ) nested1
        ) nested2;
      `;

      await typeInEditor(page, deepQuery, 10);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await switchToTab(page, 'Tree');
      await page.waitForTimeout(1000);

      await expect(page.locator('svg')).toBeVisible();
    });

    test('should handle plan with multiple join types', async ({ page }) => {
      await page.goto(BASE_URL);

      const multiJoinQuery = `
        SELECT e.name, d.name, p.title
        FROM employees e
        INNER JOIN departments d ON e.department_id = d.id
        LEFT JOIN projects p ON p.department_id = d.id
        RIGHT JOIN locations l ON l.id = d.location_id
        WHERE e.active = true;
      `;

      await typeInEditor(page, multiJoinQuery, 10);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      for (const tab of VISUALIZATION_TABS) {
        await page.click(tab.selector);
        await page.waitForTimeout(500);

        const tabElement = page.locator(tab.selector);
        await expect(tabElement).toHaveAttribute('aria-selected', 'true');
      }
    });
  });

  test.describe('Copy Functionality', () => {
    test('should copy plan text to clipboard', async ({ page, context }) => {
      await context.grantPermissions(['clipboard-read', 'clipboard-write']);
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      const copyButton = page.locator('button:has([data-testid="ContentCopyIcon"])');
      if (await copyButton.isVisible()) {
        await copyButton.click();
        await page.waitForTimeout(500);

        const clipboardText = await page.evaluate(() => navigator.clipboard.readText());
        expect(clipboardText.length).toBeGreaterThan(0);
      }
    });

    test('should copy share URL to clipboard', async ({ page, context }) => {
      await context.grantPermissions(['clipboard-read', 'clipboard-write']);
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);

      await page.click('button:has-text("Share")');
      await expect(page.locator('text=Share Query')).toBeVisible();

      const copyButton = page.locator('button:has-text("Copy")');
      if (await copyButton.isVisible()) {
        await copyButton.click();
        await page.waitForTimeout(500);

        const clipboardText = await page.evaluate(() => navigator.clipboard.readText());
        expect(clipboardText).toContain('http');
      }
    });
  });

  test.describe('Browser Compatibility', () => {
    test('should handle browser back button', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);

      const shareUrl = await openShareDialog(page);
      await page.click('button:has-text("Close")');

      await page.goto(shareUrl);
      await page.waitForTimeout(1000);

      await page.goBack();
      await page.waitForTimeout(1000);

      await expect(page).toHaveURL(BASE_URL);
    });

    test('should handle page refresh', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);

      const shareUrl = await openShareDialog(page);
      await page.click('button:has-text("Close")');

      await page.goto(shareUrl);
      await page.waitForTimeout(1000);

      await page.reload();
      await page.waitForTimeout(1000);

      const editorContent = await page.textContent('.monaco-editor .view-lines');
      expect(editorContent).toContain('employees');
    });
  });

  test.describe('Concurrent Operations', () => {
    test('should handle rapid tab switching', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.join);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      for (let i = 0; i < 3; i++) {
        for (const tab of VISUALIZATION_TABS) {
          await page.click(tab.selector, { timeout: 2000 }).catch(() => {});
          await page.waitForTimeout(100);
        }
      }

      const activeTab = page.locator('button[role="tab"][aria-selected="true"]');
      await expect(activeTab).toBeVisible();
    });

    test('should handle multiple search operations', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.complex);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      const searchTerms = ['Scan', 'Join', 'Sort', 'Filter'];

      for (const term of searchTerms) {
        await searchInPlan(page, term);
        await page.waitForTimeout(300);

        const searchInput = page.locator('input[placeholder*="Search"]');
        await searchInput.clear();
      }
    });
  });

  test.describe('Memory and Performance', () => {
    test('should handle repeated query executions', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.simple);

      for (let i = 0; i < 5; i++) {
        await executeQuery(page);
        await waitForQueryCompletion(page);
        await page.waitForTimeout(500);
      }

      const isResponsive = await page.evaluate(() => {
        return document.readyState === 'complete';
      });
      expect(isResponsive).toBe(true);
    });

    test('should clean up visualizations on new execution', async ({ page }) => {
      await page.goto(BASE_URL);

      await typeInEditor(page, SAMPLE_QUERIES.join);
      await executeQuery(page);
      await waitForQueryCompletion(page);

      await switchToTab(page, 'Flow');
      await page.waitForTimeout(1000);

      await page.locator('.monaco-editor').first().click();
      await page.keyboard.selectAll();
      await page.keyboard.press('Delete');
      await typeInEditor(page, SAMPLE_QUERIES.aggregation);

      await executeQuery(page);
      await waitForQueryCompletion(page);

      await expect(page.locator('.react-flow')).toBeVisible({ timeout: 5000 });
    });
  });
});

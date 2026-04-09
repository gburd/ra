import { type Page } from '@playwright/test';

export const SAMPLE_QUERIES = {
  simple: 'SELECT * FROM employees WHERE id = 1;',
  join: 'SELECT e.name, d.name FROM employees e JOIN departments d ON e.department_id = d.id;',
  aggregation: 'SELECT department_id, COUNT(*) as count FROM employees GROUP BY department_id;',
  subquery: 'SELECT * FROM employees WHERE salary > (SELECT AVG(salary) FROM employees);',
  complex: `
    SELECT e.*, d.name as dept_name, m.name as manager_name
    FROM employees e
    LEFT JOIN departments d ON e.department_id = d.id
    LEFT JOIN employees m ON d.manager_id = m.id
    WHERE e.hire_date > '2020-01-01'
      AND e.salary > 50000
    ORDER BY e.salary DESC
    LIMIT 100;
  `,
};

export async function typeInEditor(page: Page, text: string, delay = 30): Promise<void> {
  await page.locator('.monaco-editor').click();
  await page.keyboard.type(text, { delay });
}

export async function executeQuery(page: Page): Promise<void> {
  await page.click('button:has-text("Execute")');
}

export async function waitForQueryCompletion(page: Page, timeout = 30000): Promise<void> {
  await page.waitForSelector('[role="progressbar"]', { state: 'visible', timeout: 5000 }).catch(() => {});
  await page.waitForSelector('[role="progressbar"]', { state: 'hidden', timeout });
}

export async function switchToTab(page: Page, tabName: string): Promise<void> {
  await page.click(`button[role="tab"]:has-text("${tabName}")`);
  await page.waitForTimeout(500);
}

export async function openSchemaViewer(page: Page): Promise<void> {
  await page.click('button:has([data-testid="SchemaIcon"])');
  await page.waitForSelector('text=Available Schemas', { state: 'visible' });
}

export async function selectSchema(page: Page, schemaName: string, queryName: string): Promise<void> {
  await openSchemaViewer(page);
  await page.click(`text=${schemaName}`);
  await page.click(`text=${queryName}`);
  await page.click('button:has-text("Close")');
}

export async function addPanel(page: Page): Promise<void> {
  await page.click('button:has-text("Add Panel")');
  await page.waitForTimeout(500);
}

export async function changeEngine(page: Page, panelIndex: number, engineName: string): Promise<void> {
  const engineSelectors = page.locator('select, [role="combobox"]').filter({ hasText: /PostgreSQL|MySQL|DuckDB/ });
  await engineSelectors.nth(panelIndex).click();
  await page.click(`text=${engineName}`);
}

export async function openShareDialog(page: Page): Promise<string> {
  await page.click('button:has-text("Share")');
  await page.waitForSelector('text=Share Query', { state: 'visible' });
  const urlInput = page.locator('input[readonly]');
  return await urlInput.inputValue();
}

export async function openSearch(page: Page): Promise<void> {
  await page.click('button:has([data-testid="SearchIcon"])');
  await page.waitForSelector('input[placeholder*="Search"]', { state: 'visible' });
}

export async function searchInPlan(page: Page, searchTerm: string): Promise<void> {
  await openSearch(page);
  const searchInput = page.locator('input[placeholder*="Search"]');
  await searchInput.fill(searchTerm);
  await page.keyboard.press('Enter');
  await page.waitForTimeout(500);
}

export async function navigateSearchResults(page: Page, direction: 'next' | 'previous'): Promise<void> {
  const buttonLocator = direction === 'next'
    ? page.locator('button[aria-label*="Next"]').or(page.locator('button:has-text("Next")'))
    : page.locator('button[aria-label*="Previous"]').or(page.locator('button:has-text("Previous")'));

  if (await buttonLocator.isVisible()) {
    await buttonLocator.click();
    await page.waitForTimeout(300);
  }
}

export async function clearSearch(page: Page): Promise<void> {
  const searchInput = page.locator('input[placeholder*="Search"]');
  if (await searchInput.isVisible()) {
    await searchInput.clear();
    await page.keyboard.press('Escape');
  }
}

export async function getEditorContent(page: Page): Promise<string> {
  return await page.textContent('.monaco-editor .view-lines') || '';
}

export async function verifyTabRendered(page: Page, tabName: string): Promise<boolean> {
  await switchToTab(page, tabName);

  switch (tabName) {
    case 'Raw':
      return await page.locator('.monaco-editor').isVisible();
    case 'Tree':
      return await page.locator('svg').isVisible();
    case 'Flow':
      return await page.locator('.react-flow').isVisible();
    case 'Cost':
      return await page.locator('text=Total Cost').or(page.locator('text=Operation Breakdown')).isVisible();
    case 'Warnings':
      return await page.locator('text=No warnings detected').or(page.locator('[data-testid="warning-item"]')).isVisible();
    default:
      return false;
  }
}

export async function setExplainMode(page: Page, mode: 'explain' | 'analyze'): Promise<void> {
  const modeButton = page.locator(`button:has-text("${mode === 'explain' ? 'Explain' : 'Analyze'}")`);
  await modeButton.click();
}

export async function getPanelCount(page: Page): Promise<number> {
  const engineSelectors = page.locator('select, [role="combobox"]').filter({ hasText: /PostgreSQL|MySQL|DuckDB/ });
  return await engineSelectors.count();
}

export async function verifyPanelHasResults(page: Page, panelIndex: number): Promise<boolean> {
  const rawPlanTabs = page.locator('button[role="tab"]:has-text("Raw")');
  const tab = rawPlanTabs.nth(panelIndex);
  await tab.click();
  return await page.locator('.monaco-editor').nth(panelIndex + 1).isVisible();
}

export async function takeScreenshotOnFailure(page: Page, testName: string): Promise<void> {
  await page.screenshot({ path: `test-results/${testName}-failure.png`, fullPage: true });
}

export const VISUALIZATION_TABS = [
  { name: 'Raw', selector: 'button[role="tab"]:has-text("Raw")' },
  { name: 'Tree', selector: 'button[role="tab"]:has-text("Tree")' },
  { name: 'Flow', selector: 'button[role="tab"]:has-text("Flow")' },
  { name: 'Cost', selector: 'button[role="tab"]:has-text("Cost")' },
  { name: 'Warnings', selector: 'button[role="tab"]:has-text("Warnings")' },
];

export const ENGINES = [
  { id: 'postgresql-15', name: 'PostgreSQL 15' },
  { id: 'postgresql-16', name: 'PostgreSQL 16' },
  { id: 'postgresql-17', name: 'PostgreSQL 17' },
  { id: 'mysql-8.0', name: 'MySQL 8.0' },
  { id: 'mysql-8.4', name: 'MySQL 8.4' },
  { id: 'mariadb-11', name: 'MariaDB 11' },
  { id: 'duckdb', name: 'DuckDB' },
  { id: 'sqlite', name: 'SQLite' },
];

export const SCHEMAS = [
  { name: 'HR (Employee-Department)', query: 'Find High Earners' },
  { name: 'E-Commerce', query: 'Recent Orders' },
  { name: 'TPC-H (Benchmark)', query: 'Revenue by Order Priority' },
  { name: 'Sakila (DVD Rental)', query: 'Most Popular Films' },
  { name: 'Blog Platform', query: 'Recent Published Posts' },
];

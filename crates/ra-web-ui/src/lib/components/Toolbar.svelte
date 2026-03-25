<script lang="ts">
  interface Props {
    onrun: () => void;
    onvisualize: () => void;
    oncompare: () => void;
    onsample: (sql: string) => void;
    running: boolean;
  }

  let { onrun, onvisualize, oncompare, onsample, running }: Props =
    $props();

  const SAMPLE_QUERIES = [
    {
      label: "Simple SELECT",
      sql: "SELECT * FROM users WHERE id > 10;",
    },
    {
      label: "JOIN query",
      sql: `SELECT u.name, o.total
FROM users u
JOIN orders o ON u.id = o.user_id
WHERE o.total > 100;`,
    },
    {
      label: "Aggregate",
      sql: `SELECT category, COUNT(*) as cnt, AVG(price) as avg_price
FROM products
GROUP BY category
ORDER BY cnt DESC;`,
    },
    {
      label: "Subquery",
      sql: `SELECT name, email
FROM users
WHERE id IN (
  SELECT user_id FROM orders
  WHERE total > 500
);`,
    },
    {
      label: "Complex JOIN",
      sql: `SELECT u.name, p.name as product, oi.quantity, oi.price
FROM users u
JOIN orders o ON u.id = o.user_id
JOIN order_items oi ON o.id = oi.order_id
JOIN products p ON oi.product_id = p.id
WHERE o.status = 'completed'
ORDER BY o.created_at DESC
LIMIT 20;`,
    },
  ];

  let selectEl: HTMLSelectElement;
</script>

<div class="toolbar">
  <div class="toolbar-left">
    <button class="run-btn" onclick={onrun} disabled={running}>
      {running ? "Running..." : "Run (Ctrl+Enter)"}
    </button>
    <button class="action-btn" onclick={onvisualize} disabled={running}>
      Visualize Plan
    </button>
    <button class="action-btn" onclick={oncompare} disabled={running}>
      Compare Plans
    </button>
  </div>

  <div class="toolbar-right">
    <select
      class="sample-select"
      bind:this={selectEl}
      onchange={() => {
        const idx = Number(selectEl.value);
        const query = SAMPLE_QUERIES[idx];
        if (query) {
          onsample(query.sql);
        }
        selectEl.value = "";
      }}
    >
      <option value="" disabled selected>Load sample...</option>
      {#each SAMPLE_QUERIES as query, i}
        <option value={i}>{query.label}</option>
      {/each}
    </select>
  </div>
</div>

<style>
  .toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    gap: 8px;
    flex-shrink: 0;
  }

  .toolbar-left,
  .toolbar-right {
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .run-btn {
    padding: 6px 16px;
    background: var(--green);
    color: var(--bg-primary);
    border-radius: var(--radius-sm);
    font-weight: 600;
    font-size: 13px;
    transition: all 0.15s ease;
  }

  .run-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .run-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-btn {
    padding: 6px 12px;
    background: var(--bg-surface);
    color: var(--text-primary);
    border-radius: var(--radius-sm);
    font-size: 13px;
    transition: all 0.15s ease;
  }

  .action-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .sample-select {
    padding: 5px 8px;
    background: var(--bg-surface);
    color: var(--text-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
  }

  .sample-select:focus {
    outline: 1px solid var(--accent);
  }
</style>

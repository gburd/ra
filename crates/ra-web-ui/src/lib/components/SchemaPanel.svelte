<script lang="ts">
  interface Props {
    onexecute: (sql: string) => void;
  }

  let { onexecute }: Props = $props();

  const SAMPLE_SCHEMAS = [
    {
      name: "E-Commerce",
      sql: `CREATE TABLE users (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  email TEXT UNIQUE,
  created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE orders (
  id INTEGER PRIMARY KEY,
  user_id INTEGER REFERENCES users(id),
  total REAL NOT NULL,
  status TEXT DEFAULT 'pending',
  created_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE products (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  price REAL NOT NULL,
  category TEXT
);

CREATE TABLE order_items (
  id INTEGER PRIMARY KEY,
  order_id INTEGER REFERENCES orders(id),
  product_id INTEGER REFERENCES products(id),
  quantity INTEGER NOT NULL,
  price REAL NOT NULL
);`,
    },
    {
      name: "Analytics",
      sql: `CREATE TABLE events (
  id INTEGER PRIMARY KEY,
  user_id INTEGER,
  event_type TEXT NOT NULL,
  properties TEXT,
  timestamp TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  user_id INTEGER,
  start_time TEXT,
  end_time TEXT,
  page_count INTEGER
);`,
    },
  ];

  let selectedSchema = $state(0);

  function applySchema(): void {
    const schema = SAMPLE_SCHEMAS[selectedSchema];
    if (schema) {
      onexecute(schema.sql);
    }
  }
</script>

<div class="schema-panel">
  <div class="panel-header">Schema</div>

  <div class="schema-options">
    {#each SAMPLE_SCHEMAS as schema, i}
      <button
        class="schema-btn"
        class:active={selectedSchema === i}
        onclick={() => {
          selectedSchema = i;
        }}
      >
        {schema.name}
      </button>
    {/each}
  </div>

  <button class="apply-btn" onclick={applySchema}>
    Apply Schema
  </button>

  <div class="schema-preview">
    <pre>{SAMPLE_SCHEMAS[selectedSchema]?.sql ?? ""}</pre>
  </div>
</div>

<style>
  .schema-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
  }

  .panel-header {
    font-weight: 600;
    font-size: 13px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 0 4px;
  }

  .schema-options {
    display: flex;
    gap: 4px;
  }

  .schema-btn {
    padding: 4px 10px;
    background: var(--bg-surface);
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    font-size: 12px;
    transition: all 0.15s ease;
  }

  .schema-btn:hover {
    background: var(--bg-hover);
  }

  .schema-btn.active {
    background: var(--accent);
    color: var(--bg-primary);
  }

  .apply-btn {
    padding: 6px 12px;
    background: var(--accent);
    color: var(--bg-primary);
    border-radius: var(--radius-sm);
    font-size: 12px;
    font-weight: 600;
    transition: background 0.15s ease;
  }

  .apply-btn:hover {
    background: var(--accent-hover);
  }

  .schema-preview {
    flex: 1;
    overflow: auto;
    background: var(--bg-secondary);
    border-radius: var(--radius-sm);
    padding: 8px;
  }

  pre {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>

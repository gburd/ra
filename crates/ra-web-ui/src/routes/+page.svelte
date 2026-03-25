<script lang="ts">
  import Editor from "$lib/components/Editor.svelte";
  import Toolbar from "$lib/components/Toolbar.svelte";
  import PlanTree from "$lib/components/PlanTree.svelte";
  import ResultsTable from "$lib/components/ResultsTable.svelte";
  import SchemaPanel from "$lib/components/SchemaPanel.svelte";
  import RulesPanel from "$lib/components/RulesPanel.svelte";
  import ComparePlans from "$lib/components/ComparePlans.svelte";
  import { visualize, comparePlans } from "$lib/api/client";
  import type {
    VisualizeResponse,
    ComparePlansResponse,
  } from "$lib/api/client";
  import { executeSQL, resetDb } from "$lib/api/sqljsdb";
  import type { QueryResult } from "$lib/api/sqljsdb";

  let sql = $state(
    "SELECT u.name, o.total\nFROM users u\nJOIN orders o ON u.id = o.user_id\nWHERE o.total > 100\nORDER BY o.total DESC\nLIMIT 10;",
  );

  let running = $state(false);
  let error = $state("");

  type ActiveTab = "results" | "plan" | "compare";
  let activeTab = $state<ActiveTab>("results");

  let queryResult = $state<QueryResult | null>(null);
  let planResult = $state<VisualizeResponse | null>(null);
  let compareResult = $state<ComparePlansResponse | null>(null);
  let rulesApplied = $state<string[]>([]);

  const HISTORY_KEY = "ra-query-history";
  let history = $state<string[]>(loadHistory());

  function loadHistory(): string[] {
    if (typeof localStorage === "undefined") return [];
    const raw = localStorage.getItem(HISTORY_KEY);
    if (!raw) return [];
    try {
      const parsed: unknown = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        return parsed.filter(
          (item): item is string => typeof item === "string",
        );
      }
      return [];
    } catch {
      return [];
    }
  }

  function saveToHistory(query: string): void {
    const trimmed = query.trim();
    if (!trimmed) return;
    history = [trimmed, ...history.filter((h) => h !== trimmed)].slice(
      0,
      20,
    );
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
    }
  }

  async function handleRun(): Promise<void> {
    const trimmed = sql.trim();
    if (!trimmed) return;

    running = true;
    error = "";
    activeTab = "results";

    try {
      const result = await executeSQL(trimmed);
      queryResult = result;
      saveToHistory(trimmed);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      queryResult = null;
    } finally {
      running = false;
    }
  }

  async function handleVisualize(): Promise<void> {
    const trimmed = sql.trim();
    if (!trimmed) return;

    running = true;
    error = "";
    activeTab = "plan";

    try {
      const result = await visualize(trimmed);
      planResult = result;
      rulesApplied = result.rules_applied;
      saveToHistory(trimmed);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      planResult = null;
    } finally {
      running = false;
    }
  }

  async function handleCompare(): Promise<void> {
    const trimmed = sql.trim();
    if (!trimmed) return;

    running = true;
    error = "";
    activeTab = "compare";

    try {
      const result = await comparePlans(trimmed);
      compareResult = result;
      saveToHistory(trimmed);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      compareResult = null;
    } finally {
      running = false;
    }
  }

  function handleSchemaExecute(schemaSql: string): void {
    resetDb();
    executeSQL(schemaSql)
      .then(() => {
        error = "";
      })
      .catch((e: unknown) => {
        error = `Schema error: ${e instanceof Error ? e.message : String(e)}`;
      });
  }

  function loadSample(querySql: string): void {
    sql = querySql;
  }
</script>

<svelte:head>
  <title>RA Query Explorer</title>
</svelte:head>

<div class="page-layout">
  <Toolbar
    onrun={handleRun}
    onvisualize={handleVisualize}
    oncompare={handleCompare}
    onsample={loadSample}
    {running}
  />

  <div class="content-area">
    <aside class="sidebar">
      <SchemaPanel onexecute={handleSchemaExecute} />
    </aside>

    <div class="main-panels">
      <div class="editor-panel">
        <Editor
          bind:value={sql}
          onrun={handleRun}
        />
      </div>

      <div class="output-panel">
        {#if error}
          <div class="error-bar">{error}</div>
        {/if}

        <div class="tab-bar">
          <button
            class="tab"
            class:active={activeTab === "results"}
            onclick={() => {
              activeTab = "results";
            }}
          >
            Results
          </button>
          <button
            class="tab"
            class:active={activeTab === "plan"}
            onclick={() => {
              activeTab = "plan";
            }}
          >
            Plan
          </button>
          <button
            class="tab"
            class:active={activeTab === "compare"}
            onclick={() => {
              activeTab = "compare";
            }}
          >
            Compare
          </button>
        </div>

        <div class="tab-content">
          {#if activeTab === "results"}
            <ResultsTable
              columns={queryResult?.columns ?? []}
              rows={queryResult?.rows ?? []}
              timeMs={queryResult?.timeMs}
            />
          {:else if activeTab === "plan"}
            {#if planResult}
              <div class="plan-layout">
                <div class="plan-tree-scroll">
                  <PlanTree
                    node={planResult.plan}
                    totalCost={planResult.total_cost}
                  />
                </div>
                <div class="rules-sidebar">
                  <RulesPanel rules={rulesApplied} />
                </div>
              </div>
            {:else}
              <div class="placeholder">
                Click "Visualize Plan" to see the query plan.
              </div>
            {/if}
          {:else if activeTab === "compare"}
            <ComparePlans data={compareResult} />
          {/if}
        </div>
      </div>
    </div>
  </div>

  {#if history.length > 0}
    <div class="history-bar">
      <span class="history-label">History:</span>
      {#each history.slice(0, 5) as item}
        <button
          class="history-item"
          onclick={() => loadSample(item)}
          title={item}
        >
          {item.slice(0, 40)}{item.length > 40 ? "..." : ""}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .page-layout {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .content-area {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .sidebar {
    width: 260px;
    background: var(--bg-secondary);
    border-right: 1px solid var(--border);
    padding: 12px;
    overflow-y: auto;
    flex-shrink: 0;
  }

  .main-panels {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .editor-panel {
    height: 40%;
    min-height: 150px;
    border-bottom: 1px solid var(--border);
  }

  .output-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .error-bar {
    padding: 6px 12px;
    background: color-mix(in srgb, var(--red) 15%, transparent);
    color: var(--red);
    font-family: var(--font-mono);
    font-size: 12px;
    border-bottom: 1px solid var(--red);
    flex-shrink: 0;
  }

  .tab-bar {
    display: flex;
    gap: 0;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .tab {
    padding: 8px 16px;
    background: none;
    color: var(--text-muted);
    font-size: 13px;
    border-bottom: 2px solid transparent;
    transition: all 0.15s ease;
  }

  .tab:hover {
    color: var(--text-secondary);
  }

  .tab.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent);
  }

  .tab-content {
    flex: 1;
    overflow: hidden;
    padding: 8px;
  }

  .plan-layout {
    display: flex;
    height: 100%;
    gap: 8px;
  }

  .plan-tree-scroll {
    flex: 1;
    overflow: auto;
    padding: 8px;
    background: var(--bg-secondary);
    border-radius: var(--radius);
    border: 1px solid var(--border);
  }

  .rules-sidebar {
    width: 200px;
    flex-shrink: 0;
    padding: 8px;
    background: var(--bg-secondary);
    border-radius: var(--radius);
    border: 1px solid var(--border);
    overflow-y: auto;
  }

  .placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: 13px;
  }

  .history-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    background: var(--bg-secondary);
    border-top: 1px solid var(--border);
    overflow-x: auto;
    flex-shrink: 0;
  }

  .history-label {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .history-item {
    padding: 2px 8px;
    background: var(--bg-surface);
    color: var(--text-secondary);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 11px;
    white-space: nowrap;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: background 0.1s ease;
    flex-shrink: 0;
  }

  .history-item:hover {
    background: var(--bg-hover);
  }

  @media (max-width: 768px) {
    .sidebar {
      display: none;
    }

    .plan-layout {
      flex-direction: column;
    }

    .rules-sidebar {
      width: 100%;
      max-height: 150px;
    }
  }
</style>

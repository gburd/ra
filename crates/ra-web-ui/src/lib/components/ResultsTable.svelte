<script lang="ts">
  interface Props {
    columns: string[];
    rows: string[][];
    timeMs?: number;
  }

  let { columns, rows, timeMs }: Props = $props();
</script>

<div class="results-container">
  {#if columns.length === 0 && rows.length === 0}
    <div class="empty">No results. Run a query to see output.</div>
  {:else}
    <div class="results-meta">
      <span>{rows.length} row{rows.length !== 1 ? "s" : ""}</span>
      {#if timeMs !== undefined}
        <span class="time">{timeMs.toFixed(1)}ms</span>
      {/if}
    </div>
    <div class="table-scroll">
      <table>
        <thead>
          <tr>
            {#each columns as col}
              <th>{col}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each rows as row}
            <tr>
              {#each row as cell}
                <td class:null-cell={cell === "NULL"}>{cell}</td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .results-container {
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: 13px;
  }

  .results-meta {
    display: flex;
    justify-content: space-between;
    padding: 6px 12px;
    font-size: 12px;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .time {
    font-family: var(--font-mono);
    color: var(--green);
  }

  .table-scroll {
    overflow: auto;
    flex: 1;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-family: var(--font-mono);
    font-size: 13px;
  }

  th {
    position: sticky;
    top: 0;
    background: var(--bg-surface);
    padding: 6px 12px;
    text-align: left;
    font-weight: 600;
    color: var(--text-primary);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }

  td {
    padding: 4px 12px;
    border-bottom: 1px solid var(--bg-surface);
    white-space: nowrap;
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  tr:hover td {
    background: var(--bg-hover);
  }

  .null-cell {
    color: var(--text-muted);
    font-style: italic;
  }
</style>

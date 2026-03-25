<script lang="ts">
  import type { VisualPlanNode } from "$lib/api/client";

  interface Props {
    plan: VisualPlanNode | null;
    totalCost: number;
  }

  let { plan, totalCost }: Props = $props();

  interface CostEntry {
    operator: string;
    cost: number;
    pct: number;
    rows: number;
  }

  function flattenCosts(
    node: VisualPlanNode | null,
    total: number,
  ): CostEntry[] {
    if (!node) return [];
    const entries: CostEntry[] = [];
    collectCosts(node, total, entries);
    entries.sort((a, b) => b.cost - a.cost);
    return entries;
  }

  function collectCosts(
    node: VisualPlanNode,
    total: number,
    acc: CostEntry[],
  ): void {
    acc.push({
      operator: `${node.operator_type} (${node.id})`,
      cost: node.cost,
      pct: total > 0 ? (node.cost / total) * 100 : 0,
      rows: node.rows,
    });
    for (const child of node.children) {
      collectCosts(child, total, acc);
    }
  }

  const entries = $derived(flattenCosts(plan, totalCost));
</script>

<div class="cost-breakdown">
  <div class="panel-header">
    Cost Breakdown
    {#if totalCost > 0}
      <span class="total">Total: {totalCost.toFixed(1)}</span>
    {/if}
  </div>

  {#if entries.length === 0}
    <div class="empty">No plan available.</div>
  {:else}
    <div class="entries">
      {#each entries as entry}
        <div class="entry">
          <div class="entry-header">
            <span class="op-name">{entry.operator}</span>
            <span class="op-cost">{entry.cost.toFixed(1)}</span>
          </div>
          <div class="bar-container">
            <div
              class="bar-fill"
              style="width: {Math.min(entry.pct, 100)}%"
            ></div>
          </div>
          <div class="entry-meta">
            <span>{entry.pct.toFixed(1)}%</span>
            <span>{entry.rows.toLocaleString()} rows</span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .cost-breakdown {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-weight: 600;
    font-size: 13px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 0 4px 6px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .total {
    font-family: var(--font-mono);
    font-size: 12px;
    text-transform: none;
    letter-spacing: 0;
    color: var(--accent);
  }

  .empty {
    padding: 12px 4px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .entries {
    overflow-y: auto;
    flex: 1;
    padding: 4px 0;
  }

  .entry {
    padding: 4px;
    border-radius: var(--radius-sm);
    transition: background 0.1s ease;
  }

  .entry:hover {
    background: var(--bg-hover);
  }

  .entry-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 2px;
  }

  .op-name {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .op-cost {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .bar-container {
    height: 4px;
    background: var(--bg-surface);
    border-radius: 2px;
    overflow: hidden;
  }

  .bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .entry-meta {
    display: flex;
    justify-content: space-between;
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-muted);
    margin-top: 1px;
  }
</style>

<script lang="ts">
  import type { VisualPlanNode } from "$lib/api/client";
  import PlanTreeSelf from "./PlanTree.svelte";

  interface Props {
    node: VisualPlanNode;
    depth?: number;
    totalCost?: number;
  }

  let { node, depth = 0, totalCost = 0 }: Props = $props();

  const costPct = totalCost > 0 ? (node.cost / totalCost) * 100 : 0;

  function formatRows(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return String(n);
  }

  function operatorColor(op: string): string {
    const lower = op.toLowerCase();
    if (lower.includes("scan")) return "var(--green)";
    if (lower.includes("join") || lower.includes("loop"))
      return "var(--accent)";
    if (lower.includes("sort") || lower.includes("aggregate"))
      return "var(--yellow)";
    if (lower.includes("filter") || lower.includes("where"))
      return "var(--peach)";
    return "var(--text-primary)";
  }
</script>

<div class="plan-node" style="margin-left: {depth * 24}px">
  <div class="node-header">
    <span class="operator" style="color: {operatorColor(node.operator_type)}">
      {node.operator_type}
    </span>
    <span class="cost" title="Estimated cost">
      {node.cost.toFixed(1)}
    </span>
    {#if costPct > 0}
      <span class="cost-bar">
        <span
          class="cost-fill"
          style="width: {Math.min(costPct, 100)}%"
        ></span>
      </span>
    {/if}
  </div>

  <div class="node-meta">
    <span class="rows">{formatRows(node.rows)} rows</span>
    {#each node.details as detail}
      <span class="detail" title={detail.key}>
        {detail.key}: {detail.value}
      </span>
    {/each}
  </div>

  {#if node.children.length > 0}
    <div class="children">
      {#each node.children as child}
        <PlanTreeSelf node={child} depth={depth + 1} {totalCost} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .plan-node {
    border-left: 2px solid var(--border);
    padding: 6px 0 6px 12px;
    margin: 2px 0;
  }

  .plan-node:hover {
    background: var(--bg-hover);
    border-radius: var(--radius-sm);
  }

  .node-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .operator {
    font-family: var(--font-mono);
    font-weight: 600;
    font-size: 13px;
  }

  .cost {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-muted);
  }

  .cost-bar {
    flex: 1;
    max-width: 80px;
    height: 4px;
    background: var(--bg-surface);
    border-radius: 2px;
    overflow: hidden;
  }

  .cost-fill {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .node-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 2px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .rows {
    font-family: var(--font-mono);
  }

  .detail {
    font-family: var(--font-mono);
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .children {
    margin-top: 4px;
  }
</style>

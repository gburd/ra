<script lang="ts">
  import type { ComparePlansResponse } from "$lib/api/client";
  import PlanTree from "./PlanTree.svelte";

  interface Props {
    data: ComparePlansResponse | null;
  }

  let { data }: Props = $props();
</script>

{#if data}
  <div class="compare-container">
    <div class="summary-bar">
      <span>Cheapest: <strong>{data.summary.cheapest}</strong></span>
      <div class="cost-badges">
        {#each data.summary.costs as cost}
          <span
            class="cost-badge"
            class:cheapest={cost.optimizer === data.summary.cheapest}
          >
            {cost.optimizer}: {cost.total_cost.toFixed(1)}
            ({cost.node_count} nodes)
          </span>
        {/each}
      </div>
    </div>

    <div class="plans-grid">
      {#each data.plans as plan}
        <div class="plan-column">
          <div class="plan-title">
            <span class="optimizer-name">{plan.optimizer}</span>
            <span class="plan-cost">{plan.total_cost.toFixed(1)}</span>
          </div>
          <div class="plan-scroll">
            <PlanTree node={plan.plan} totalCost={plan.total_cost} />
          </div>
        </div>
      {/each}
    </div>
  </div>
{:else}
  <div class="empty">Click "Compare Plans" to see plan differences.</div>
{/if}

<style>
  .compare-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .summary-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: var(--bg-surface);
    border-radius: var(--radius-sm);
    font-size: 13px;
    flex-shrink: 0;
    margin-bottom: 8px;
  }

  .cost-badges {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .cost-badge {
    padding: 2px 8px;
    border-radius: 12px;
    background: var(--bg-hover);
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
  }

  .cost-badge.cheapest {
    background: var(--green);
    color: var(--bg-primary);
  }

  .plans-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 8px;
    flex: 1;
    overflow: auto;
  }

  .plan-column {
    background: var(--bg-secondary);
    border-radius: var(--radius);
    border: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .plan-title {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .optimizer-name {
    font-weight: 600;
    font-size: 14px;
  }

  .plan-cost {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-muted);
  }

  .plan-scroll {
    overflow: auto;
    padding: 8px;
    flex: 1;
  }

  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>

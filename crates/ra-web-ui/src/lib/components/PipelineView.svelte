<script lang="ts">
  import type { VisualPlanNode } from "$lib/api/client";
  import PlanTree from "./PlanTree.svelte";

  interface PipelineStage {
    name: string;
    plan: VisualPlanNode;
    cost: number;
    label: string;
  }

  interface Props {
    stages: PipelineStage[];
  }

  let { stages }: Props = $props();

  function improvementPct(
    original: number,
    optimized: number,
  ): string {
    if (original <= 0) return "N/A";
    const pct = ((original - optimized) / original) * 100;
    return `${pct.toFixed(1)}%`;
  }
</script>

{#if stages.length === 0}
  <div class="empty">
    Click "Visualize Plan" to see the optimization pipeline.
  </div>
{:else}
  <div class="pipeline-container">
    <div class="pipeline-header">
      {#each stages as stage, i}
        <div class="stage-tab" class:first={i === 0}>
          <span class="stage-name">{stage.name}</span>
          <span class="stage-cost">{stage.cost.toFixed(1)}</span>
          {#if i > 0 && stages[0]}
            <span class="stage-improvement">
              {improvementPct(stages[0].cost, stage.cost)} reduction
            </span>
          {/if}
        </div>
        {#if i < stages.length - 1}
          <span class="arrow">-&gt;</span>
        {/if}
      {/each}
    </div>

    <div class="stages-grid">
      {#each stages as stage}
        <div class="stage-column">
          <div class="stage-title">
            <span>{stage.name}</span>
            <span class="stage-label">{stage.label}</span>
          </div>
          <div class="stage-plan">
            <PlanTree node={stage.plan} totalCost={stage.cost} />
          </div>
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .pipeline-container {
    display: flex;
    flex-direction: column;
    height: 100%;
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

  .pipeline-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-surface);
    border-radius: var(--radius-sm);
    margin-bottom: 8px;
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .stage-tab {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 6px 16px;
    background: var(--bg-hover);
    border-radius: var(--radius-sm);
    min-width: 100px;
  }

  .stage-tab.first {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
  }

  .stage-name {
    font-weight: 600;
    font-size: 12px;
    color: var(--text-primary);
  }

  .stage-cost {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
  }

  .stage-improvement {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--green);
  }

  .arrow {
    color: var(--text-muted);
    font-size: 16px;
    flex-shrink: 0;
  }

  .stages-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 8px;
    flex: 1;
    overflow: auto;
  }

  .stage-column {
    background: var(--bg-secondary);
    border-radius: var(--radius);
    border: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .stage-title {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    font-weight: 600;
    font-size: 13px;
  }

  .stage-label {
    font-weight: 400;
    font-size: 11px;
    color: var(--text-muted);
  }

  .stage-plan {
    overflow: auto;
    padding: 8px;
    flex: 1;
  }
</style>

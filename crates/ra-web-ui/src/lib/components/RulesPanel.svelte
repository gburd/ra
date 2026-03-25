<script lang="ts">
  interface Props {
    rules: string[];
  }

  let { rules }: Props = $props();
</script>

<div class="rules-panel">
  <div class="panel-header">
    Rules Applied ({rules.length})
  </div>
  {#if rules.length === 0}
    <div class="empty">No rules applied yet.</div>
  {:else}
    <ul class="rules-list">
      {#each rules as rule, i}
        <li class="rule-item">
          <span class="rule-index">{i + 1}</span>
          <span class="rule-name">{rule}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .rules-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .panel-header {
    font-weight: 600;
    font-size: 13px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 0 4px 6px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .empty {
    padding: 12px 4px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .rules-list {
    list-style: none;
    overflow-y: auto;
    flex: 1;
  }

  .rule-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px;
    border-radius: var(--radius-sm);
    transition: background 0.1s ease;
  }

  .rule-item:hover {
    background: var(--bg-hover);
  }

  .rule-index {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
    width: 20px;
    text-align: right;
    flex-shrink: 0;
  }

  .rule-name {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--accent);
  }
</style>

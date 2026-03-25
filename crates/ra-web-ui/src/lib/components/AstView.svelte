<script lang="ts">
  interface Props {
    ast: unknown;
  }

  let { ast }: Props = $props();

  function formatJson(obj: unknown): string {
    return JSON.stringify(obj, null, 2);
  }

  function getNodeType(obj: unknown): string {
    if (obj === null || obj === undefined) return "null";
    if (typeof obj !== "object") return typeof obj;
    if (Array.isArray(obj)) return "array";
    const keys = Object.keys(obj as Record<string, unknown>);
    if (keys.length === 1 && keys[0]) return keys[0];
    return "object";
  }

  function getNodeChildren(
    obj: unknown,
  ): Array<{ key: string; value: unknown }> {
    if (obj === null || obj === undefined || typeof obj !== "object")
      return [];
    if (Array.isArray(obj))
      return obj.map((v, i) => ({ key: String(i), value: v }));

    const entries = Object.entries(obj as Record<string, unknown>);
    if (entries.length === 1 && entries[0]) {
      const inner = entries[0][1];
      if (inner && typeof inner === "object" && !Array.isArray(inner)) {
        return Object.entries(inner as Record<string, unknown>).map(
          ([k, v]) => ({ key: k, value: v }),
        );
      }
    }
    return entries.map(([k, v]) => ({ key: k, value: v }));
  }

  function isPrimitive(obj: unknown): boolean {
    return obj === null || typeof obj !== "object";
  }

  function formatPrimitive(obj: unknown): string {
    if (obj === null) return "null";
    if (typeof obj === "string") return `"${obj}"`;
    return String(obj);
  }
</script>

<div class="ast-view">
  {#if ast === null || ast === undefined}
    <div class="empty">
      Click "Visualize Plan" to see the parsed AST.
    </div>
  {:else}
    <div class="ast-header">
      <span class="ast-label">Relational Algebra Expression</span>
    </div>
    <div class="ast-content">
      <pre class="json-view">{formatJson(ast)}</pre>
    </div>
  {/if}
</div>

<style>
  .ast-view {
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

  .ast-header {
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .ast-label {
    font-weight: 600;
    font-size: 13px;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .ast-content {
    flex: 1;
    overflow: auto;
    padding: 12px;
    background: var(--bg-secondary);
  }

  .json-view {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-primary);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.6;
  }
</style>

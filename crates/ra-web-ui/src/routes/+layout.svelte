<script lang="ts">
  import "../app.css";

  import type { Snippet } from "svelte";

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();
</script>

<div class="app-shell">
  <header class="app-header">
    <div class="header-left">
      <span class="logo">RA</span>
      <span class="title">Query Explorer</span>
    </div>
    <nav class="header-nav">
      <a href="/" class="nav-link active">Editor</a>
    </nav>
  </header>

  <main class="app-main">
    {@render children()}
  </main>
</div>

<style>
  .app-shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }

  .app-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0 16px;
    height: 44px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .logo {
    font-family: var(--font-mono);
    font-weight: 800;
    font-size: 18px;
    color: var(--accent);
    background: var(--bg-surface);
    padding: 2px 8px;
    border-radius: var(--radius-sm);
  }

  .title {
    font-weight: 600;
    font-size: 14px;
    color: var(--text-secondary);
  }

  .header-nav {
    display: flex;
    gap: 4px;
  }

  .nav-link {
    padding: 4px 12px;
    border-radius: var(--radius-sm);
    text-decoration: none;
    font-size: 13px;
    color: var(--text-muted);
    transition: all 0.15s ease;
  }

  .nav-link:hover {
    color: var(--text-primary);
    background: var(--bg-surface);
  }

  .nav-link.active {
    color: var(--text-primary);
    background: var(--bg-surface);
  }

  .app-main {
    flex: 1;
    overflow: hidden;
  }
</style>

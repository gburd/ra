<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type * as Monaco from "monaco-editor";

  interface Props {
    value: string;
    onchange?: (value: string) => void;
    onrun?: () => void;
  }

  let { value = $bindable(), onchange, onrun }: Props = $props();

  let container: HTMLDivElement;
  let editor: Monaco.editor.IStandaloneCodeEditor | undefined;
  let monaco: typeof Monaco | undefined;

  onMount(async () => {
    monaco = await import("monaco-editor");

    // Configure Monaco environment for web workers.
    // In browser builds, we use the bundled editor without separate workers.
    self.MonacoEnvironment = {
      getWorker: () =>
        new Worker(
          new URL(
            "monaco-editor/esm/vs/editor/editor.worker.js",
            import.meta.url,
          ),
          { type: "module" },
        ),
    };

    monaco.editor.defineTheme("ra-dark", {
      base: "vs-dark",
      inherit: true,
      rules: [
        { token: "keyword", foreground: "cba6f7" },
        { token: "string", foreground: "a6e3a1" },
        { token: "number", foreground: "fab387" },
        { token: "comment", foreground: "6c7086", fontStyle: "italic" },
        { token: "operator", foreground: "89dceb" },
        { token: "type", foreground: "f9e2af" },
      ],
      colors: {
        "editor.background": "#1e1e2e",
        "editor.foreground": "#cdd6f4",
        "editor.lineHighlightBackground": "#313244",
        "editor.selectionBackground": "#45475a",
        "editorCursor.foreground": "#f5e0dc",
        "editorLineNumber.foreground": "#6c7086",
        "editorLineNumber.activeForeground": "#cdd6f4",
      },
    });

    editor = monaco.editor.create(container, {
      value,
      language: "sql",
      theme: "ra-dark",
      fontSize: 14,
      fontFamily: "var(--font-mono)",
      lineNumbers: "on",
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      automaticLayout: true,
      tabSize: 2,
      wordWrap: "on",
      padding: { top: 12, bottom: 12 },
      renderLineHighlight: "line",
      suggestOnTriggerCharacters: true,
      quickSuggestions: true,
    });

    editor.onDidChangeModelContent(() => {
      const newValue = editor?.getValue() ?? "";
      value = newValue;
      onchange?.(newValue);
    });

    editor.addAction({
      id: "run-query",
      label: "Run Query",
      keybindings: [
        monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter,
      ],
      run: () => onrun?.(),
    });
  });

  onDestroy(() => {
    editor?.dispose();
  });

  export function setValue(newValue: string): void {
    if (editor && editor.getValue() !== newValue) {
      editor.setValue(newValue);
    }
  }
</script>

<div class="editor-container" bind:this={container}></div>

<style>
  .editor-container {
    width: 100%;
    height: 100%;
    min-height: 200px;
    border-radius: var(--radius);
    overflow: hidden;
  }
</style>

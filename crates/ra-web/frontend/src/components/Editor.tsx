import { useRef, useEffect } from 'react';
import MonacoEditor, { type OnMount } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';

declare global {
  interface Window {
    monaco: typeof import('monaco-editor');
  }
}

interface EditorProps {
  value: string;
  onChange: (value: string) => void;
  onExecute: () => void;
}

export function Editor({ value, onChange, onExecute }: EditorProps) {
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);

  const handleEditorDidMount: OnMount = (editor) => {
    editorRef.current = editor;

    editor.addCommand(
      // Ctrl+Enter or Cmd+Enter
      // eslint-disable-next-line no-bitwise
      window.monaco.KeyMod.CtrlCmd | window.monaco.KeyCode.Enter,
      () => {
        onExecute();
      }
    );
  };

  const handleEditorChange = (newValue: string | undefined) => {
    onChange(newValue || '');
  };

  useEffect(() => {
    return () => {
      editorRef.current?.dispose();
    };
  }, []);

  return (
    <MonacoEditor
      height="100%"
      defaultLanguage="sql"
      value={value}
      onChange={handleEditorChange}
      onMount={handleEditorDidMount}
      theme="vs-dark"
      options={{
        minimap: { enabled: false },
        fontSize: 14,
        lineNumbers: 'on',
        renderLineHighlight: 'all',
        scrollBeyondLastLine: false,
        automaticLayout: true,
        tabSize: 2,
        wordWrap: 'on',
      }}
    />
  );
}

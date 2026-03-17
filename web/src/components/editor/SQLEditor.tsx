import { useRef, useEffect, useCallback } from "preact/hooks";

interface SQLEditorProps {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly readonly?: boolean;
  readonly placeholder?: string;
}

/**
 * SQL editor component using a plain textarea.
 *
 * This provides a functional editing experience without pulling
 * in Monaco Editor's large bundle. Monaco can be added later as
 * an optional enhancement loaded via dynamic import.
 */
export function SQLEditor({
  value,
  onChange,
  readonly: isReadonly = false,
  placeholder = "-- Write your SQL here...",
}: SQLEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleInput = useCallback(
    (e: Event) => {
      const target = e.target as HTMLTextAreaElement;
      onChange(target.value);
    },
    [onChange],
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // Tab inserts two spaces instead of moving focus
      if (e.key === "Tab") {
        e.preventDefault();
        const textarea = textareaRef.current;
        if (textarea === null) return;

        const start = textarea.selectionStart;
        const end = textarea.selectionEnd;
        const before = value.slice(0, start);
        const after = value.slice(end);
        const updated = `${before}  ${after}`;
        onChange(updated);

        // Restore cursor position after the inserted spaces
        requestAnimationFrame(() => {
          textarea.selectionStart = start + 2;
          textarea.selectionEnd = start + 2;
        });
      }
    },
    [value, onChange],
  );

  // Auto-resize textarea to fit content
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea === null) return;
    textarea.style.height = "auto";
    textarea.style.height = `${String(textarea.scrollHeight)}px`;
  }, [value]);

  return (
    <div class="sql-editor">
      <textarea
        ref={textareaRef}
        class="sql-textarea"
        value={value}
        onInput={handleInput}
        onKeyDown={handleKeyDown}
        readOnly={isReadonly}
        placeholder={placeholder}
        spellcheck={false}
        autocomplete="off"
        autocapitalize="off"
      />
    </div>
  );
}

import { useState, useCallback } from "preact/hooks";

interface SpecFileEditorProps {
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly readonly?: boolean;
}

interface ParsedSection {
  readonly kind: "setup" | "teardown" | "session" | "permutation" | "comment";
  readonly name?: string;
  readonly content: string;
  readonly line: number;
}

/**
 * Editor for `.spec` isolation test files.
 *
 * Renders the raw text in a textarea with a sidebar showing
 * parsed sections (setup, teardown, sessions, permutations)
 * for quick navigation.
 */
export function SpecFileEditor({
  value,
  onChange,
  readonly: isReadonly = false,
}: SpecFileEditorProps) {
  const [activeLine, setActiveLine] = useState(0);
  const sections = parseSections(value);

  const handleInput = useCallback(
    (e: Event) => {
      const target = e.target as HTMLTextAreaElement;
      onChange(target.value);
    },
    [onChange],
  );

  const jumpToLine = useCallback(
    (line: number) => {
      setActiveLine(line);
    },
    [],
  );

  return (
    <div class="spec-editor">
      <div class="spec-sidebar">
        <h4 class="spec-sidebar-title">Sections</h4>
        {sections.map((section, i) => (
          <button
            key={i}
            class={`spec-section-btn ${
              activeLine === section.line ? "active" : ""
            }`}
            onClick={() => jumpToLine(section.line)}
          >
            <span class={`section-kind ${section.kind}`}>
              {section.kind}
            </span>
            {section.name !== undefined && (
              <span class="section-name">{section.name}</span>
            )}
          </button>
        ))}
      </div>
      <div class="spec-editor-main">
        <textarea
          class="spec-textarea"
          value={value}
          onInput={handleInput}
          readOnly={isReadonly}
          spellcheck={false}
          autocomplete="off"
          autocapitalize="off"
        />
      </div>
    </div>
  );
}

function parseSections(text: string): readonly ParsedSection[] {
  const sections: ParsedSection[] = [];
  const lines = text.split("\n");

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]?.trim() ?? "";

    if (line.startsWith("setup")) {
      sections.push({ kind: "setup", content: "", line: i + 1 });
    } else if (line.startsWith("teardown")) {
      sections.push({ kind: "teardown", content: "", line: i + 1 });
    } else if (line.startsWith("session")) {
      const nameMatch = line.match(/session\s+"([^"]+)"/);
      const sessionName = nameMatch?.[1];
      const sessionSection: ParsedSection = sessionName !== undefined
        ? { kind: "session", name: sessionName, content: "", line: i + 1 }
        : { kind: "session", content: "", line: i + 1 };
      sections.push(sessionSection);
    } else if (line.startsWith("permutation")) {
      sections.push({
        kind: "permutation",
        content: "",
        line: i + 1,
      });
    }
  }

  return sections;
}

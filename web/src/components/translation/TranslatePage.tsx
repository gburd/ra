import { useState, useCallback } from "preact/hooks";
import type { DatabaseId, TranslateResponse } from "src/types.ts";
import { DATABASES } from "src/types.ts";
import { translateSQL } from "src/api/client.ts";
import { SQLEditor } from "src/components/editor/SQLEditor.tsx";

interface TranslatePageProps {
  readonly path: string;
}

const ALL_DIALECTS = DATABASES.map((db) => db.id);

export function TranslatePage(_props: TranslatePageProps) {
  const [sql, setSql] = useState(
    "SELECT first_name || ' ' || last_name AS full_name\n" +
      "FROM users\n" +
      "WHERE created_at > NOW() - INTERVAL '7 days'\n" +
      "LIMIT 10;",
  );
  const [source, setSource] = useState<DatabaseId>("postgresql");
  const [target, setTarget] = useState<DatabaseId>("mysql");
  const [result, setResult] = useState<TranslateResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleTranslate = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await translateSQL({ sql, source, target });
      setResult(response);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [sql, source, target]);

  const handleSwap = useCallback(() => {
    setSource(target);
    setTarget(source);
    if (result !== null) {
      setSql(result.translated_sql);
      setResult(null);
    }
  }, [source, target, result]);

  return (
    <div class="translate-page">
      <div class="translate-header">
        <h2>SQL Dialect Translation</h2>
        <p class="translate-desc">
          Translate SQL between database dialects. Differences in syntax,
          functions, and operators are handled automatically.
        </p>
      </div>

      <div class="translate-controls">
        <DialectSelect
          label="From"
          value={source}
          onChange={setSource}
          exclude={target}
        />
        <button
          class="btn btn-secondary btn-swap"
          onClick={handleSwap}
          title="Swap source and target"
        >
          &#x21C4;
        </button>
        <DialectSelect
          label="To"
          value={target}
          onChange={setTarget}
          exclude={source}
        />
        <button
          class="btn btn-primary"
          onClick={handleTranslate}
          disabled={loading || sql.trim() === ""}
        >
          {loading ? "Translating..." : "Translate"}
        </button>
      </div>

      <div class="translate-panels">
        <div class="translate-panel">
          <div class="translate-panel-label">
            Source ({dialectName(source)})
          </div>
          <SQLEditor
            value={sql}
            onChange={setSql}
            placeholder="-- Enter SQL in the source dialect..."
          />
        </div>

        <div class="translate-panel">
          <div class="translate-panel-label">
            Target ({dialectName(target)})
          </div>
          {result !== null ? (
            <SQLEditor
              value={result.translated_sql}
              onChange={() => {}}
              readonly={true}
            />
          ) : (
            <div class="translate-placeholder">
              Translated SQL will appear here.
            </div>
          )}
        </div>
      </div>

      {error !== null && <div class="error-banner">{error}</div>}

      {result !== null && result.warnings.length > 0 && (
        <div class="translate-warnings">
          <h3>Warnings</h3>
          <ul>
            {result.warnings.map((w, i) => (
              <li key={i} class="warning-item">
                {w}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

interface DialectSelectProps {
  readonly label: string;
  readonly value: DatabaseId;
  readonly onChange: (db: DatabaseId) => void;
  readonly exclude: DatabaseId;
}

function DialectSelect({
  label,
  value,
  onChange,
  exclude,
}: DialectSelectProps) {
  return (
    <label class="dialect-select">
      <span class="dialect-label">{label}</span>
      <select
        class="config-select"
        value={value}
        onChange={(e) =>
          onChange(
            (e.target as HTMLSelectElement).value as DatabaseId,
          )
        }
      >
        {ALL_DIALECTS.map((id) => (
          <option key={id} value={id} disabled={id === exclude}>
            {dialectName(id)}
          </option>
        ))}
      </select>
    </label>
  );
}

function dialectName(id: DatabaseId): string {
  const db = DATABASES.find((d) => d.id === id);
  return db?.name ?? id;
}

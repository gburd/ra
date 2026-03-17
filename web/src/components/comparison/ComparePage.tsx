import { useState, useCallback } from "preact/hooks";
import type { DatabaseId, ExecuteResponse } from "src/types.ts";
import { DATABASES } from "src/types.ts";
import { executeSQL } from "src/api/client.ts";
import { SQLEditor } from "src/components/editor/SQLEditor.tsx";
import { ResultTable } from "src/components/shared/ResultTable.tsx";

interface ComparePageProps {
  readonly path: string;
}

interface CompareResult {
  readonly database: DatabaseId;
  readonly response?: ExecuteResponse;
  readonly error?: string;
  readonly loading: boolean;
}

export function ComparePage(_props: ComparePageProps) {
  const [sql, setSql] = useState("SELECT 1 + 1 AS result;");
  const [selectedDbs, setSelectedDbs] = useState<DatabaseId[]>([
    "sqlite",
    "duckdb",
  ]);
  const [results, setResults] = useState<CompareResult[]>([]);

  const toggleDb = useCallback(
    (db: DatabaseId) => {
      setSelectedDbs((prev) =>
        prev.includes(db) ? prev.filter((d) => d !== db) : [...prev, db],
      );
    },
    [],
  );

  const handleCompare = useCallback(async () => {
    const initial: CompareResult[] = selectedDbs.map((db) => ({
      database: db,
      loading: true,
    }));
    setResults(initial);

    const settled = await Promise.allSettled(
      selectedDbs.map(async (db) => {
        const response = await executeSQL({ sql, database: db });
        return { database: db, response };
      }),
    );

    const final: CompareResult[] = settled.map((s, i) => {
      const db = selectedDbs[i];
      if (db === undefined) {
        return { database: "sqlite", loading: false, error: "unknown db" };
      }
      if (s.status === "fulfilled") {
        return {
          database: db,
          response: s.value.response,
          loading: false,
        };
      }
      return {
        database: db,
        error: s.reason instanceof Error ? s.reason.message : String(s.reason),
        loading: false,
      };
    });
    setResults(final);
  }, [sql, selectedDbs]);

  const availableDbs = DATABASES.filter((db) => db.available);

  return (
    <div class="compare-page">
      <div class="compare-editor">
        <div class="compare-toolbar">
          <div class="db-toggles">
            {availableDbs.map((db) => (
              <label key={db.id} class="db-toggle">
                <input
                  type="checkbox"
                  checked={selectedDbs.includes(db.id)}
                  onChange={() => toggleDb(db.id)}
                />
                <span style={{ color: db.color }}>{db.name}</span>
              </label>
            ))}
          </div>
          <button
            class="btn btn-primary"
            onClick={handleCompare}
            disabled={selectedDbs.length === 0}
          >
            Compare
          </button>
        </div>
        <SQLEditor value={sql} onChange={setSql} />
      </div>

      <div class="compare-results">
        {results.map((r) => (
          <CompareResultPanel key={r.database} result={r} />
        ))}
      </div>
    </div>
  );
}

interface CompareResultPanelProps {
  readonly result: CompareResult;
}

function CompareResultPanel({ result }: CompareResultPanelProps) {
  const dbInfo = DATABASES.find((d) => d.id === result.database);
  const dbName = dbInfo?.name ?? result.database;
  const dbColor = dbInfo?.color ?? "#666";

  return (
    <div class="compare-panel" style={{ borderTopColor: dbColor }}>
      <div class="compare-panel-header">
        <span class="compare-db-name" style={{ color: dbColor }}>
          {dbName}
        </span>
        {result.loading && <span class="loading-indicator">Running...</span>}
      </div>
      {result.error !== undefined && (
        <div class="error-banner">{result.error}</div>
      )}
      {result.response !== undefined && (
        <ResultTable
          columns={result.response.columns}
          rows={result.response.rows}
          elapsed_ms={result.response.elapsed_ms}
        />
      )}
    </div>
  );
}

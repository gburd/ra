import { useState, useCallback, useEffect } from "preact/hooks";
import type {
  DatabaseId,
  ExecuteResponse,
  ExplainResponse,
} from "src/types.ts";
import { executeSQL, explainSQL } from "src/api/client.ts";
import { SQLEditor } from "src/components/editor/SQLEditor.tsx";
import { ResultTable } from "src/components/shared/ResultTable.tsx";
import { PlanViewer } from "src/components/visualization/PlanViewer.tsx";
import { ShareButton } from "src/components/shared/ShareButton.tsx";

interface EditorPageProps {
  readonly path: string;
  readonly database: DatabaseId;
}

type Tab = "results" | "plan" | "rules";

export function EditorPage({ database }: EditorPageProps) {
  const [sql, setSql] = useState("SELECT * FROM users WHERE age > 18;");
  const [result, setResult] = useState<ExecuteResponse | null>(null);
  const [plan, setPlan] = useState<ExplainResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [activeTab, setActiveTab] = useState<Tab>("results");

  const handleExecute = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await executeSQL({ sql, database });
      setResult(response);
      setActiveTab("results");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [sql, database]);

  const handleExplain = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await explainSQL({ sql, database, explain: true });
      setPlan(response);
      setActiveTab("plan");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [sql, database]);

  // Keyboard shortcuts: Ctrl+Enter to execute, Ctrl+Shift+Enter to explain
  useEffect(() => {
    function handleKeyboard(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
        e.preventDefault();
        if (e.shiftKey) {
          void handleExplain();
        } else {
          void handleExecute();
        }
      }
    }
    window.addEventListener("keydown", handleKeyboard);
    return () => window.removeEventListener("keydown", handleKeyboard);
  }, [handleExecute, handleExplain]);

  const getShareState = useCallback(
    () => ({ sql, database, tab: activeTab }),
    [sql, database, activeTab],
  );

  return (
    <div class="editor-page">
      <div class="editor-panel">
        <div class="editor-toolbar">
          <button
            class="btn btn-primary"
            onClick={handleExecute}
            disabled={loading}
            title="Execute query (Ctrl+Enter)"
          >
            {loading ? "Running..." : "Execute"}
          </button>
          <button
            class="btn btn-secondary"
            onClick={handleExplain}
            disabled={loading}
            title="Explain query plan (Ctrl+Shift+Enter)"
          >
            Explain
          </button>
          <span class="db-label">{database}</span>
          <ShareButton getState={getShareState} />
        </div>
        <SQLEditor value={sql} onChange={setSql} />
      </div>

      <div class="result-panel">
        <div class="tab-bar">
          <button
            class={`tab ${activeTab === "results" ? "active" : ""}`}
            onClick={() => setActiveTab("results")}
          >
            Results
            {result ? ` (${result.row_count} rows)` : ""}
          </button>
          <button
            class={`tab ${activeTab === "plan" ? "active" : ""}`}
            onClick={() => setActiveTab("plan")}
          >
            Query Plan
          </button>
          <button
            class={`tab ${activeTab === "rules" ? "active" : ""}`}
            onClick={() => setActiveTab("rules")}
          >
            Rules Applied
          </button>
        </div>

        {error !== null && <div class="error-banner">{error}</div>}

        <div class="tab-content">
          {activeTab === "results" && result !== null && (
            <ResultTable
              columns={result.columns}
              rows={result.rows}
              elapsed_ms={result.elapsed_ms}
            />
          )}
          {activeTab === "plan" && plan !== null && (
            <PlanViewer plan={plan.plan} />
          )}
          {activeTab === "rules" && plan !== null && (
            <RulesList rules={plan.rules_applied} />
          )}
        </div>
      </div>
    </div>
  );
}

interface RulesListProps {
  readonly rules: readonly string[];
}

function RulesList({ rules }: RulesListProps) {
  if (rules.length === 0) {
    return <p class="empty-state">No optimization rules were applied.</p>;
  }

  return (
    <div class="rules-list">
      <h3>Optimization Rules Applied</h3>
      <ol>
        {rules.map((rule, i) => (
          <li key={i} class="rule-item">
            {rule}
          </li>
        ))}
      </ol>
    </div>
  );
}

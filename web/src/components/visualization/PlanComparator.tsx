import { useState, useCallback } from "preact/hooks";
import { SQLEditor } from "src/components/editor/SQLEditor.tsx";
import { PlanVisualizer } from "src/components/visualization/PlanVisualizer.tsx";
import type { VisualPlanNode } from "src/components/visualization/PlanVisualizer.tsx";
import { comparePlans } from "src/api/client.ts";

interface PlanComparatorProps {
  readonly path: string;
}

/** Plan returned per optimizer from the API. */
interface OptimizerPlan {
  readonly optimizer: string;
  readonly plan: VisualPlanNode;
  readonly total_cost: number;
  readonly available: boolean;
}

/** Cost summary from comparison API. */
interface CostSummary {
  readonly cheapest: string;
  readonly costs: readonly OptimizerCostEntry[];
}

interface OptimizerCostEntry {
  readonly optimizer: string;
  readonly total_cost: number;
  readonly node_count: number;
}

/** Full response from /api/compare-plans. */
interface ComparePlansResponse {
  readonly plans: readonly OptimizerPlan[];
  readonly summary: CostSummary;
}

const OPTIMIZER_COLORS: Record<string, string> = {
  Ra: "#4a9eff",
  PostgreSQL: "#336791",
  MySQL: "#4479A1",
  DuckDB: "#FFF000",
};

const SAMPLE_QUERIES = [
  {
    label: "Simple SELECT",
    sql: "SELECT * FROM users WHERE age > 25;",
  },
  {
    label: "JOIN query",
    sql: [
      "SELECT u.name, o.total",
      "FROM users u",
      "JOIN orders o ON u.id = o.user_id",
      "WHERE o.total > 100;",
    ].join("\n"),
  },
  {
    label: "Aggregation",
    sql: [
      "SELECT department, COUNT(*), AVG(salary)",
      "FROM employees",
      "WHERE status = 'active'",
      "GROUP BY department",
      "ORDER BY COUNT(*) DESC;",
    ].join("\n"),
  },
  {
    label: "Complex JOIN",
    sql: [
      "SELECT c.name, p.title, o.quantity",
      "FROM customers c",
      "JOIN orders o ON c.id = o.customer_id",
      "JOIN products p ON o.product_id = p.id",
      "WHERE c.country = 'US'",
      "ORDER BY o.quantity DESC;",
    ].join("\n"),
  },
];

export function PlanComparator(_props: PlanComparatorProps) {
  const [sql, setSql] = useState(SAMPLE_QUERIES[0]?.sql ?? "");
  const [result, setResult] = useState<ComparePlansResponse | null>(
    null,
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [highlightedNode, setHighlightedNode] = useState<
    string | null
  >(null);

  const handleCompare = useCallback(async () => {
    if (sql.trim() === "") return;
    setLoading(true);
    setError(null);
    setResult(null);

    try {
      const resp =
        await comparePlans<ComparePlansResponse>({
          sql,
        });
      setResult(resp);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : String(err),
      );
    } finally {
      setLoading(false);
    }
  }, [sql]);

  const handleSampleSelect = useCallback(
    (e: Event) => {
      const target = e.target as HTMLSelectElement;
      const idx = Number.parseInt(target.value, 10);
      const query = SAMPLE_QUERIES[idx];
      if (query !== undefined) {
        setSql(query.sql);
      }
    },
    [],
  );

  const handleNodeHover = useCallback(
    (nodeId: string | null) => {
      setHighlightedNode(nodeId);
    },
    [],
  );

  return (
    <div class="comparator-page">
      <div class="comparator-header">
        <h2>Plan Comparison</h2>
        <p class="comparator-desc">
          Compare query execution plans across Ra, PostgreSQL, MySQL,
          and DuckDB optimizers side-by-side.
        </p>
      </div>

      <div class="comparator-editor-section">
        <div class="comparator-toolbar">
          <div class="comparator-sample-select">
            <label class="comparator-sample-label">
              Sample:
            </label>
            <select
              class="config-select"
              onChange={handleSampleSelect}
            >
              {SAMPLE_QUERIES.map((q, i) => (
                <option key={q.label} value={i}>
                  {q.label}
                </option>
              ))}
            </select>
          </div>
          <button
            class="btn btn-primary"
            onClick={handleCompare}
            disabled={loading || sql.trim() === ""}
          >
            {loading ? "Comparing..." : "Compare Plans"}
          </button>
        </div>
        <SQLEditor value={sql} onChange={setSql} />
      </div>

      {error !== null && (
        <div class="error-banner">{error}</div>
      )}

      {result !== null && (
        <>
          <div class="comparator-grid">
            {result.plans.map((p) => (
              <div
                class="comparator-panel"
                key={p.optimizer}
                style={{
                  borderTopColor:
                    OPTIMIZER_COLORS[p.optimizer] ?? "var(--border)",
                }}
              >
                <div class="comparator-panel-header">
                  <span
                    class="comparator-panel-name"
                    style={{
                      color:
                        OPTIMIZER_COLORS[p.optimizer] ??
                        "var(--text)",
                    }}
                  >
                    {p.optimizer}
                  </span>
                  <span class="comparator-panel-cost">
                    cost: {p.total_cost.toFixed(1)}
                  </span>
                  {result.summary.cheapest === p.optimizer && (
                    <span class="comparator-cheapest-badge">
                      cheapest
                    </span>
                  )}
                </div>
                <div class="comparator-panel-body">
                  <PlanVisualizer
                    plan={p.plan}
                    highlightedNodeId={highlightedNode}
                    onNodeHover={handleNodeHover}
                    compact
                  />
                </div>
              </div>
            ))}
          </div>

          <CostComparisonTable summary={result.summary} />
        </>
      )}
    </div>
  );
}

interface CostComparisonTableProps {
  readonly summary: CostSummary;
}

function CostComparisonTable({ summary }: CostComparisonTableProps) {
  const maxCost = Math.max(
    ...summary.costs.map((c) => c.total_cost),
  );

  return (
    <div class="comparator-cost-table">
      <h3>Cost Comparison</h3>
      <table class="demo-table">
        <thead>
          <tr>
            <th>Optimizer</th>
            <th>Total Cost</th>
            <th>Nodes</th>
            <th>Relative Cost</th>
          </tr>
        </thead>
        <tbody>
          {summary.costs.map((c) => {
            const pct =
              maxCost > 0 ? (c.total_cost / maxCost) * 100 : 0;
            const isCheapest =
              c.optimizer === summary.cheapest;
            return (
              <tr
                key={c.optimizer}
                class={isCheapest ? "comparator-cheapest-row" : ""}
              >
                <td>
                  <span
                    style={{
                      color:
                        OPTIMIZER_COLORS[c.optimizer] ??
                        "var(--text)",
                      fontWeight: 600,
                    }}
                  >
                    {c.optimizer}
                  </span>
                </td>
                <td class="mono">{c.total_cost.toFixed(1)}</td>
                <td class="mono">{c.node_count}</td>
                <td>
                  <div class="comparator-cost-bar-wrap">
                    <div
                      class="comparator-cost-bar"
                      style={{
                        width: `${String(pct)}%`,
                        background:
                          OPTIMIZER_COLORS[c.optimizer] ??
                          "var(--accent)",
                      }}
                    />
                    <span class="comparator-cost-pct">
                      {pct.toFixed(0)}%
                    </span>
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

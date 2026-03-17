import { useState } from "preact/hooks";
import type { PlanNode } from "src/types.ts";

interface PlanViewerProps {
  readonly plan: PlanNode;
}

/**
 * Renders a query plan as an interactive tree.
 *
 * Each node shows the operator name, estimated rows, cost,
 * and any properties. Nodes can be expanded/collapsed.
 * Uses pure CSS for the tree layout without D3.
 */
export function PlanViewer({ plan }: PlanViewerProps) {
  return (
    <div class="plan-viewer">
      <PlanTreeNode node={plan} depth={0} />
    </div>
  );
}

interface PlanTreeNodeProps {
  readonly node: PlanNode;
  readonly depth: number;
}

function PlanTreeNode({ node, depth }: PlanTreeNodeProps) {
  const [expanded, setExpanded] = useState(true);
  const hasChildren = node.children.length > 0;
  const properties = Object.entries(node.properties);

  return (
    <div class="plan-node" style={{ marginLeft: `${String(depth * 24)}px` }}>
      <div
        class="plan-node-header"
        onClick={() => setExpanded(!expanded)}
        role="button"
        tabIndex={0}
      >
        {hasChildren && (
          <span class="expand-icon">{expanded ? "\u25BC" : "\u25B6"}</span>
        )}
        <span class="operator-name">{node.operator}</span>
        <span class="node-stats">
          <span class="stat" title="Estimated rows">
            ~{formatNumber(node.estimated_rows)} rows
          </span>
          {node.actual_rows !== undefined && (
            <span
              class={`stat actual ${cardinalityClass(
                node.estimated_rows,
                node.actual_rows,
              )}`}
              title="Actual rows"
            >
              {formatNumber(node.actual_rows)} actual
            </span>
          )}
          <span class="stat" title="Cost">
            cost: {node.cost.toFixed(1)}
          </span>
        </span>
      </div>

      {expanded && properties.length > 0 && (
        <div class="plan-node-properties">
          {properties.map(([key, val]) => (
            <span key={key} class="property">
              <span class="prop-key">{key}:</span> {val}
            </span>
          ))}
        </div>
      )}

      {expanded &&
        node.children.map((child) => (
          <PlanTreeNode key={child.id} node={child} depth={depth + 1} />
        ))}
    </div>
  );
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(Math.round(n));
}

function cardinalityClass(estimated: number, actual: number): string {
  if (estimated === 0) return "";
  const ratio = actual / estimated;
  if (ratio > 10) return "severe-underestimate";
  if (ratio > 2) return "underestimate";
  if (ratio < 0.1) return "severe-overestimate";
  if (ratio < 0.5) return "overestimate";
  return "accurate";
}

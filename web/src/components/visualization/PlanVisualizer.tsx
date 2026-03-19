import { useState, useRef, useEffect, useMemo } from "preact/hooks";

/** Visual plan node from the API. */
export interface VisualPlanNode {
  readonly id: string;
  readonly operator_type: string;
  readonly cost: number;
  readonly rows: number;
  readonly details: readonly PlanDetail[];
  readonly children: readonly VisualPlanNode[];
  readonly position: NodePosition;
}

interface NodePosition {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

interface PlanDetail {
  readonly key: string;
  readonly value: string;
}

/** Positioned node after layout computation. */
interface LayoutNode {
  readonly id: string;
  readonly operator_type: string;
  readonly cost: number;
  readonly rows: number;
  readonly details: readonly PlanDetail[];
  readonly children: readonly LayoutNode[];
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

interface PlanVisualizerProps {
  readonly plan: VisualPlanNode;
  readonly label?: string;
  readonly highlightedNodeId?: string | null;
  readonly onNodeHover?: (nodeId: string | null) => void;
  readonly onNodeClick?: (nodeId: string) => void;
  readonly compact?: boolean;
}

const NODE_WIDTH = 180;
const NODE_HEIGHT = 64;
const H_GAP = 24;
const V_GAP = 48;
const PADDING = 40;

export function PlanVisualizer({
  plan,
  label,
  highlightedNodeId,
  onNodeHover,
  onNodeClick,
  compact = false,
}: PlanVisualizerProps) {
  const [selectedNode, setSelectedNode] = useState<string | null>(null);
  const [tooltip, setTooltip] = useState<{
    node: LayoutNode;
    x: number;
    y: number;
  } | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);

  const layout = useMemo(() => computeLayout(plan), [plan]);

  const bounds = useMemo(() => {
    let maxX = 0;
    let maxY = 0;
    visitLayout(layout, (n) => {
      const right = n.x + n.width;
      const bottom = n.y + n.height;
      if (right > maxX) maxX = right;
      if (bottom > maxY) maxY = bottom;
    });
    return {
      width: maxX + PADDING * 2,
      height: maxY + PADDING * 2,
    };
  }, [layout]);

  const handleNodeClick = (node: LayoutNode) => {
    setSelectedNode((prev) => (prev === node.id ? null : node.id));
    onNodeClick?.(node.id);
  };

  const handleNodeMouseEnter = (
    node: LayoutNode,
    evt: MouseEvent,
  ) => {
    const svg = svgRef.current;
    if (!svg) return;
    const rect = svg.getBoundingClientRect();
    setTooltip({
      node,
      x: evt.clientX - rect.left,
      y: evt.clientY - rect.top,
    });
    onNodeHover?.(node.id);
  };

  const handleNodeMouseLeave = () => {
    setTooltip(null);
    onNodeHover?.(null);
  };

  const svgWidth = compact
    ? Math.min(bounds.width, 600)
    : bounds.width;
  const svgHeight = compact
    ? Math.min(bounds.height, 400)
    : bounds.height;

  return (
    <div class="plan-visualizer">
      {label !== undefined && (
        <div class="plan-visualizer-label">{label}</div>
      )}
      <div class="plan-visualizer-svg-wrap">
        <svg
          ref={svgRef}
          width={svgWidth}
          height={svgHeight}
          viewBox={`0 0 ${String(bounds.width)} ${String(bounds.height)}`}
          class="plan-visualizer-svg"
        >
          <g transform={`translate(${String(PADDING)}, ${String(PADDING)})`}>
            <Edges node={layout} />
            <Nodes
              node={layout}
              selectedNode={selectedNode}
              highlightedNodeId={highlightedNodeId ?? null}
              onNodeClick={handleNodeClick}
              onNodeMouseEnter={handleNodeMouseEnter}
              onNodeMouseLeave={handleNodeMouseLeave}
            />
          </g>
        </svg>
        {tooltip !== null && (
          <Tooltip
            node={tooltip.node}
            x={tooltip.x}
            y={tooltip.y}
          />
        )}
      </div>
      {selectedNode !== null && (
        <NodeDetailPanel
          node={findNode(layout, selectedNode)}
          onClose={() => setSelectedNode(null)}
        />
      )}
    </div>
  );
}

interface EdgesProps {
  readonly node: LayoutNode;
}

function Edges({ node }: EdgesProps) {
  const parentCx = node.x + node.width / 2;
  const parentCy = node.y + node.height;

  return (
    <>
      {node.children.map((child) => {
        const childCx = child.x + child.width / 2;
        const childCy = child.y;
        const midY = (parentCy + childCy) / 2;
        const path = [
          `M ${String(parentCx)} ${String(parentCy)}`,
          `C ${String(parentCx)} ${String(midY)},`,
          `  ${String(childCx)} ${String(midY)},`,
          `  ${String(childCx)} ${String(childCy)}`,
        ].join(" ");
        return (
          <g key={`edge-${child.id}`}>
            <path
              d={path}
              fill="none"
              stroke="var(--border-light)"
              stroke-width="2"
            />
            <Edges node={child} />
          </g>
        );
      })}
    </>
  );
}

interface NodesProps {
  readonly node: LayoutNode;
  readonly selectedNode: string | null;
  readonly highlightedNodeId: string | null;
  readonly onNodeClick: (node: LayoutNode) => void;
  readonly onNodeMouseEnter: (
    node: LayoutNode,
    evt: MouseEvent,
  ) => void;
  readonly onNodeMouseLeave: () => void;
}

function Nodes({
  node,
  selectedNode,
  highlightedNodeId,
  onNodeClick,
  onNodeMouseEnter,
  onNodeMouseLeave,
}: NodesProps) {
  const color = costColor(node.cost);
  const isSelected = selectedNode === node.id;
  const isHighlighted = highlightedNodeId === node.id;
  const strokeColor = isSelected
    ? "var(--accent)"
    : isHighlighted
      ? "var(--accent-hover)"
      : color;
  const strokeWidth = isSelected || isHighlighted ? 3 : 2;

  return (
    <>
      <g
        class="plan-svg-node"
        style={{ cursor: "pointer" }}
        onClick={() => onNodeClick(node)}
        onMouseEnter={(e: MouseEvent) =>
          onNodeMouseEnter(node, e)
        }
        onMouseLeave={onNodeMouseLeave}
      >
        <rect
          x={node.x}
          y={node.y}
          width={node.width}
          height={node.height}
          rx="6"
          ry="6"
          fill="var(--bg-surface)"
          stroke={strokeColor}
          stroke-width={strokeWidth}
        />
        <CostIndicator
          x={node.x}
          y={node.y}
          width={node.width}
          cost={node.cost}
        />
        <text
          x={node.x + node.width / 2}
          y={node.y + 22}
          text-anchor="middle"
          fill="var(--text)"
          font-size="12"
          font-weight="600"
          font-family="var(--font-mono)"
        >
          {truncateText(node.operator_type, 20)}
        </text>
        <text
          x={node.x + node.width / 2}
          y={node.y + 40}
          text-anchor="middle"
          fill="var(--text-muted)"
          font-size="10"
          font-family="var(--font-mono)"
        >
          {formatNumber(node.rows)} rows
        </text>
        <text
          x={node.x + node.width / 2}
          y={node.y + 54}
          text-anchor="middle"
          fill={color}
          font-size="10"
          font-weight="600"
          font-family="var(--font-mono)"
        >
          cost: {node.cost.toFixed(1)}
        </text>
      </g>
      {node.children.map((child) => (
        <Nodes
          key={child.id}
          node={child}
          selectedNode={selectedNode}
          highlightedNodeId={highlightedNodeId}
          onNodeClick={onNodeClick}
          onNodeMouseEnter={onNodeMouseEnter}
          onNodeMouseLeave={onNodeMouseLeave}
        />
      ))}
    </>
  );
}

interface CostIndicatorProps {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly cost: number;
}

function CostIndicator({ x, y, width, cost }: CostIndicatorProps) {
  const color = costColor(cost);
  return (
    <rect
      x={x}
      y={y}
      width={width}
      height="4"
      rx="6"
      ry="6"
      fill={color}
      opacity="0.8"
    />
  );
}

interface TooltipProps {
  readonly node: LayoutNode;
  readonly x: number;
  readonly y: number;
}

function Tooltip({ node, x, y }: TooltipProps) {
  return (
    <div
      class="plan-tooltip"
      style={{
        left: `${String(x + 12)}px`,
        top: `${String(y - 10)}px`,
      }}
    >
      <div class="plan-tooltip-header">{node.operator_type}</div>
      <div class="plan-tooltip-row">
        <span class="plan-tooltip-key">Cost:</span>
        <span style={{ color: costColor(node.cost) }}>
          {node.cost.toFixed(2)}
        </span>
      </div>
      <div class="plan-tooltip-row">
        <span class="plan-tooltip-key">Rows:</span>
        {formatNumber(node.rows)}
      </div>
      {node.details.map((d) => (
        <div class="plan-tooltip-row" key={d.key}>
          <span class="plan-tooltip-key">{d.key}:</span>
          {d.value}
        </div>
      ))}
    </div>
  );
}

interface NodeDetailPanelProps {
  readonly node: LayoutNode | null;
  readonly onClose: () => void;
}

function NodeDetailPanel({ node, onClose }: NodeDetailPanelProps) {
  if (node === null) return null;

  return (
    <div class="plan-detail-panel">
      <div class="plan-detail-header">
        <span class="plan-detail-title">{node.operator_type}</span>
        <button class="btn-icon" onClick={onClose} title="Close">
          x
        </button>
      </div>
      <div class="plan-detail-body">
        <div class="plan-detail-stat">
          <span class="plan-detail-stat-label">Cost</span>
          <span
            class="plan-detail-stat-value"
            style={{ color: costColor(node.cost) }}
          >
            {node.cost.toFixed(2)}
          </span>
        </div>
        <div class="plan-detail-stat">
          <span class="plan-detail-stat-label">Estimated Rows</span>
          <span class="plan-detail-stat-value">
            {formatNumber(node.rows)}
          </span>
        </div>
        {node.details.length > 0 && (
          <div class="plan-detail-properties">
            <div class="plan-detail-props-label">Properties</div>
            {node.details.map((d) => (
              <div class="plan-detail-prop" key={d.key}>
                <span class="plan-detail-prop-key">{d.key}</span>
                <span class="plan-detail-prop-value">{d.value}</span>
              </div>
            ))}
          </div>
        )}
        <div class="plan-detail-stat">
          <span class="plan-detail-stat-label">Children</span>
          <span class="plan-detail-stat-value">
            {node.children.length}
          </span>
        </div>
      </div>
    </div>
  );
}

function costColor(cost: number): string {
  if (cost < 1000) return "var(--success)";
  if (cost < 10000) return "var(--warning)";
  return "var(--error)";
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(Math.round(n));
}

function truncateText(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return `${text.slice(0, maxLen - 1)}...`;
}

function computeLayout(node: VisualPlanNode): LayoutNode {
  const tree = assignWidths(node);
  assignPositions(tree, 0, 0);
  return tree;
}

interface MutableLayoutNode {
  id: string;
  operator_type: string;
  cost: number;
  rows: number;
  details: readonly PlanDetail[];
  children: MutableLayoutNode[];
  x: number;
  y: number;
  width: number;
  height: number;
  subtreeWidth: number;
}

function assignWidths(node: VisualPlanNode): MutableLayoutNode {
  const children = node.children.map(assignWidths);
  const childrenWidth =
    children.length > 0
      ? children.reduce((sum, c) => sum + c.subtreeWidth, 0) +
        (children.length - 1) * H_GAP
      : 0;
  const subtreeWidth = Math.max(NODE_WIDTH, childrenWidth);

  return {
    id: node.id,
    operator_type: node.operator_type,
    cost: node.cost,
    rows: node.rows,
    details: node.details,
    children,
    x: 0,
    y: 0,
    width: NODE_WIDTH,
    height: NODE_HEIGHT,
    subtreeWidth,
  };
}

function assignPositions(
  node: MutableLayoutNode,
  x: number,
  y: number,
): void {
  node.x = x + (node.subtreeWidth - NODE_WIDTH) / 2;
  node.y = y;

  let childX = x;
  for (const child of node.children) {
    assignPositions(child, childX, y + NODE_HEIGHT + V_GAP);
    childX += child.subtreeWidth + H_GAP;
  }
}

function visitLayout(
  node: LayoutNode,
  fn_: (n: LayoutNode) => void,
): void {
  fn_(node);
  for (const child of node.children) {
    visitLayout(child, fn_);
  }
}

function findNode(
  root: LayoutNode,
  id: string,
): LayoutNode | null {
  if (root.id === id) return root;
  for (const child of root.children) {
    const found = findNode(child, id);
    if (found !== null) return found;
  }
  return null;
}

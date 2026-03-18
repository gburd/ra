/**
 * Shared UI components for interactive demonstrations.
 */

import { useState } from "preact/hooks";
import type { CostBreakdown, SimPlanNode } from "src/components/demonstrations/types.ts";
import { formatCost } from "src/components/demonstrations/optimizer.ts";

// ---- Slider ----

interface SliderProps {
  readonly label: string;
  readonly value: number;
  readonly min: number;
  readonly max: number;
  readonly step?: number;
  readonly format?: (v: number) => string;
  readonly onChange: (v: number) => void;
}

export function Slider({
  label,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: SliderProps) {
  const display = format ? format(value) : String(value);
  return (
    <div class="demo-slider">
      <div class="demo-slider-header">
        <span class="demo-slider-label">{label}</span>
        <span class="demo-slider-value">{display}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step ?? 1}
        value={value}
        onInput={(e) => {
          onChange(Number((e.target as HTMLInputElement).value));
        }}
        class="demo-range"
      />
    </div>
  );
}

// ---- Toggle ----

interface ToggleProps {
  readonly label: string;
  readonly checked: boolean;
  readonly onChange: (v: boolean) => void;
}

export function Toggle({ label, checked, onChange }: ToggleProps) {
  return (
    <label class="demo-toggle">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => {
          onChange((e.target as HTMLInputElement).checked);
        }}
      />
      <span class="demo-toggle-label">{label}</span>
    </label>
  );
}

// ---- Select ----

interface SelectProps<T extends string> {
  readonly label: string;
  readonly value: T;
  readonly options: readonly { readonly value: T; readonly label: string }[];
  readonly onChange: (v: T) => void;
}

export function Select<T extends string>({
  label,
  value,
  options,
  onChange,
}: SelectProps<T>) {
  return (
    <div class="demo-select">
      <span class="demo-select-label">{label}</span>
      <select
        class="config-select"
        value={value}
        onChange={(e) => {
          onChange((e.target as HTMLSelectElement).value as T);
        }}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </div>
  );
}

// ---- Cost Bar Chart ----

interface CostBarProps {
  readonly costs: readonly {
    readonly label: string;
    readonly cost: CostBreakdown;
    readonly highlight?: boolean;
  }[];
}

export function CostBarChart({ costs }: CostBarProps) {
  const maxTotal = Math.max(
    ...costs.map((c) => (c.cost.total === Infinity ? 0 : c.cost.total)),
    1e-12,
  );

  return (
    <div class="demo-cost-bars">
      {costs.map((item) => {
        const pct =
          item.cost.total === Infinity
            ? 100
            : (item.cost.total / maxTotal) * 100;
        const cpuPct =
          item.cost.total > 0
            ? (item.cost.cpu / item.cost.total) * pct
            : 0;
        const ioPct =
          item.cost.total > 0
            ? (item.cost.io / item.cost.total) * pct
            : 0;
        const memPct =
          item.cost.total > 0
            ? (item.cost.memory / item.cost.total) * pct
            : 0;
        const netPct =
          item.cost.total > 0
            ? (item.cost.network / item.cost.total) * pct
            : 0;

        return (
          <div
            key={item.label}
            class={`demo-cost-row ${item.highlight ? "highlight" : ""}`}
          >
            <div class="demo-cost-label">{item.label}</div>
            <div class="demo-cost-bar-track">
              <div
                class="demo-cost-bar-seg cpu"
                style={{ width: `${cpuPct}%` }}
                title={`CPU: ${formatCost(item.cost.cpu)}`}
              />
              <div
                class="demo-cost-bar-seg io"
                style={{ width: `${ioPct}%` }}
                title={`I/O: ${formatCost(item.cost.io)}`}
              />
              <div
                class="demo-cost-bar-seg mem"
                style={{ width: `${memPct}%` }}
                title={`Memory: ${formatCost(item.cost.memory)}`}
              />
              <div
                class="demo-cost-bar-seg net"
                style={{ width: `${netPct}%` }}
                title={`Network: ${formatCost(item.cost.network)}`}
              />
            </div>
            <div class="demo-cost-total">
              {formatCost(item.cost.total)}
            </div>
          </div>
        );
      })}
      <div class="demo-cost-legend">
        <span class="legend-item">
          <span class="legend-swatch cpu" /> CPU
        </span>
        <span class="legend-item">
          <span class="legend-swatch io" /> I/O
        </span>
        <span class="legend-item">
          <span class="legend-swatch mem" /> Memory
        </span>
        <span class="legend-item">
          <span class="legend-swatch net" /> Network
        </span>
      </div>
    </div>
  );
}

// ---- Plan Tree Viewer ----

interface PlanTreeProps {
  readonly plan: SimPlanNode;
  readonly label?: string;
}

export function PlanTree({ plan, label }: PlanTreeProps) {
  return (
    <div class="demo-plan-tree">
      {label && <div class="demo-plan-tree-label">{label}</div>}
      <PlanTreeNode node={plan} depth={0} />
    </div>
  );
}

interface PlanTreeNodeProps {
  readonly node: SimPlanNode;
  readonly depth: number;
}

function PlanTreeNode({ node, depth }: PlanTreeNodeProps) {
  const [expanded, setExpanded] = useState(true);
  const hasChildren = node.children.length > 0;
  const props = Object.entries(node.properties);

  return (
    <div
      class="demo-plan-node"
      style={{ marginLeft: `${String(depth * 20)}px` }}
    >
      <div
        class="demo-plan-node-header"
        onClick={() => setExpanded(!expanded)}
        role="button"
        tabIndex={0}
      >
        {hasChildren && (
          <span class="expand-icon">
            {expanded ? "\u25BC" : "\u25B6"}
          </span>
        )}
        <span class="demo-plan-op">{node.operator}</span>
        <span class="demo-plan-stats">
          ~{formatRows(node.estimatedRows)} rows | cost:{" "}
          {formatCost(node.cost.total)}
        </span>
      </div>
      {expanded && props.length > 0 && (
        <div class="demo-plan-props">
          {props.map(([k, v]) => (
            <span key={k} class="demo-plan-prop">
              {k}: {v}
            </span>
          ))}
        </div>
      )}
      {expanded &&
        node.children.map((child, i) => (
          <PlanTreeNode key={i} node={child} depth={depth + 1} />
        ))}
    </div>
  );
}

function formatRows(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`;
  return String(Math.round(n));
}

// ---- Comparison Panel ----

interface ComparisonProps {
  readonly panels: readonly {
    readonly title: string;
    readonly plan: SimPlanNode;
    readonly badge?: string;
  }[];
}

export function ComparisonView({ panels }: ComparisonProps) {
  return (
    <div class="demo-comparison">
      {panels.map((panel) => (
        <div key={panel.title} class="demo-comparison-panel">
          <div class="demo-comparison-header">
            <span class="demo-comparison-title">{panel.title}</span>
            {panel.badge && (
              <span class="demo-comparison-badge">{panel.badge}</span>
            )}
          </div>
          <PlanTree plan={panel.plan} />
        </div>
      ))}
    </div>
  );
}

// ---- Demo wrapper ----

interface DemoCardProps {
  readonly title: string;
  readonly description: string;
  readonly children: preact.ComponentChildren;
}

export function DemoCard({ title, description, children }: DemoCardProps) {
  return (
    <div class="demo-card">
      <div class="demo-card-header">
        <h3 class="demo-card-title">{title}</h3>
        <p class="demo-card-desc">{description}</p>
      </div>
      <div class="demo-card-body">{children}</div>
    </div>
  );
}

// ---- Info Badge ----

interface BadgeProps {
  readonly label: string;
  readonly value: string;
  readonly variant?: "default" | "success" | "warning" | "error" | undefined;
}

export function Badge({ label, value, variant }: BadgeProps) {
  return (
    <span class={`demo-badge ${variant ?? "default"}`}>
      <span class="demo-badge-label">{label}:</span> {value}
    </span>
  );
}

// ---- Metric Row ----

interface MetricRowProps {
  readonly metrics: readonly {
    readonly label: string;
    readonly value: string;
    readonly variant?: "default" | "success" | "warning" | "error";
  }[];
}

export function MetricRow({ metrics }: MetricRowProps) {
  return (
    <div class="demo-metrics">
      {metrics.map((m) => (
        <Badge
          key={m.label}
          label={m.label}
          value={m.value}
          variant={m.variant}
        />
      ))}
    </div>
  );
}

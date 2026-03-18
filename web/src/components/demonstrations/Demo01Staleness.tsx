/**
 * Demo 1: Statistics Staleness Impact
 *
 * Shows how stale statistics cause the optimizer to choose different
 * (often worse) query plans by inflating or deflating cardinality
 * estimates.
 */

import { useState } from "preact/hooks";
import type { StalenessLevel } from "src/components/demonstrations/types.ts";
import {
  buildStalenessComparisonPlan,
  getHardwareProfile,
  stalenessConfidence,
  stalenessMultiplier,
} from "src/components/demonstrations/optimizer.ts";
import {
  ComparisonView,
  CostBarChart,
  DemoCard,
  MetricRow,
  Select,
  Slider,
} from "src/components/demonstrations/DemoShared.tsx";

const STALENESS_OPTIONS: readonly {
  readonly value: StalenessLevel;
  readonly label: string;
}[] = [
  { value: "fresh", label: "Fresh (< 1% change)" },
  { value: "slightly_stale", label: "Slightly Stale (1-5% change)" },
  {
    value: "moderately_stale",
    label: "Moderately Stale (5-20% change)",
  },
  { value: "very_stale", label: "Very Stale (> 20% change)" },
];

export function Demo01Staleness() {
  const [leftRows, setLeftRows] = useState(1_000_000);
  const [rightRows, setRightRows] = useState(50_000);
  const hw = getHardwareProfile("dual_socket_server");

  const levels: StalenessLevel[] = [
    "fresh",
    "slightly_stale",
    "moderately_stale",
    "very_stale",
  ];

  const plans = levels.map((staleness) => {
    const plan = buildStalenessComparisonPlan(
      hw,
      staleness,
      leftRows,
      rightRows,
    );
    const mult = stalenessMultiplier(staleness);
    const conf = stalenessConfidence(staleness);
    return { staleness, plan, mult, conf };
  });

  const comparisonPanels = plans.map((p) => ({
    title: `${p.staleness.replace(/_/g, " ")} (${(p.conf * 100).toFixed(0)}% confidence)`,
    plan: p.plan,
    badge: p.plan.operator,
  }));

  const costItems = plans.map((p) => ({
    label: p.staleness.replace(/_/g, " "),
    cost: p.plan.cost,
    highlight: p.staleness === "fresh",
  }));

  return (
    <DemoCard
      title="Statistics Staleness Impact"
      description="See how stale statistics cause the optimizer to choose different join algorithms. As data changes and statistics become outdated, cardinality estimates diverge from reality, leading to suboptimal plans."
    >
      <div class="demo-controls">
        <Slider
          label="Orders table (left)"
          value={leftRows}
          min={1000}
          max={100_000_000}
          step={1000}
          format={(v) =>
            v >= 1e6
              ? `${(v / 1e6).toFixed(1)}M rows`
              : `${(v / 1e3).toFixed(0)}K rows`
          }
          onChange={setLeftRows}
        />
        <Slider
          label="Customers table (right)"
          value={rightRows}
          min={100}
          max={10_000_000}
          step={100}
          format={(v) =>
            v >= 1e6
              ? `${(v / 1e6).toFixed(1)}M rows`
              : `${(v / 1e3).toFixed(0)}K rows`
          }
          onChange={setRightRows}
        />
      </div>

      <div class="demo-section">
        <h4>SQL Query</h4>
        <pre class="demo-sql">
          {`SELECT o.*, c.name
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.total > 100`}
        </pre>
      </div>

      <div class="demo-section">
        <h4>Cost Comparison by Staleness Level</h4>
        <CostBarChart costs={costItems} />
      </div>

      <div class="demo-section">
        <h4>Query Plans at Each Staleness Level</h4>
        <ComparisonView panels={comparisonPanels} />
      </div>

      <div class="demo-section">
        <h4>Key Insight</h4>
        <div class="demo-insight">
          With fresh statistics, the optimizer correctly estimates table
          sizes and picks Hash Join. As statistics become stale, inflated
          cardinality estimates may cause it to switch to Sort-Merge Join
          (thinking the data won't fit in memory) or even Nested Loop
          (if estimates collapse to very small). The cardinality
          multiplier for "very stale" statistics is{" "}
          {stalenessMultiplier("very_stale")}x, meaning the optimizer
          thinks the table has{" "}
          {(leftRows * stalenessMultiplier("very_stale") / 1e6).toFixed(1)}M
          rows instead of {(leftRows / 1e6).toFixed(1)}M.
        </div>
      </div>
    </DemoCard>
  );
}

/**
 * Demo 6: Subquery Unnesting
 *
 * Shows the EXISTS to SEMI JOIN transformation and its performance
 * impact. Demonstrates how the optimizer rewrites correlated
 * subqueries into more efficient join forms.
 */

import { useState } from "preact/hooks";
import type { HardwareCategory } from "src/components/demonstrations/types.ts";
import {
  formatCost,
  formatRows,
  getHardwareProfile,
  hashJoinCost,
  nestedLoopJoinCost,
  scanCost,
} from "src/components/demonstrations/optimizer.ts";
import type { CostBreakdown, SimPlanNode } from "src/components/demonstrations/types.ts";
import {
  ComparisonView,
  CostBarChart,
  DemoCard,
  Select,
  Slider,
} from "src/components/demonstrations/DemoShared.tsx";

const HW_OPTIONS: readonly {
  readonly value: HardwareCategory;
  readonly label: string;
}[] = [
  { value: "desktop_budget", label: "Desktop Budget (16GB)" },
  { value: "entry_server", label: "Entry Server (128GB)" },
  { value: "dual_socket_server", label: "Dual-Socket Server (512GB)" },
];

export function Demo06Subquery() {
  const [outerRows, setOuterRows] = useState(1_000_000);
  const [innerRows, setInnerRows] = useState(50_000);
  const [hwCategory, setHwCategory] =
    useState<HardwareCategory>("entry_server");

  const hw = getHardwareProfile(hwCategory);
  const avgRowSize = 100;

  // Correlated subquery: for each outer row, scan inner table
  const correlatedCost: CostBreakdown = {
    cpu: outerRows * innerRows * 50e-9,
    io: outerRows * (innerRows * avgRowSize) / (hw.storageBandwidthGbps * 1e9),
    memory: 0,
    network: 0,
    total:
      outerRows * innerRows * 50e-9 +
      outerRows * (innerRows * avgRowSize) / (hw.storageBandwidthGbps * 1e9),
  };

  // Semi join (hash-based)
  const semiJoinCost = hashJoinCost(
    hw,
    innerRows,
    outerRows,
    avgRowSize,
  );

  const correlatedPlan: SimPlanNode = {
    operator: "Filter (EXISTS subquery)",
    cost: correlatedCost,
    estimatedRows: Math.round(outerRows * 0.3),
    properties: { strategy: "correlated subquery" },
    children: [
      {
        operator: "Seq Scan (customers)",
        cost: scanCost(hw, outerRows, avgRowSize),
        estimatedRows: outerRows,
        properties: { table: "customers" },
        children: [],
      },
      {
        operator: "Subquery Scan (orders)",
        cost: {
          cpu: innerRows * 50e-9,
          io: (innerRows * avgRowSize) / (hw.storageBandwidthGbps * 1e9),
          memory: 0,
          network: 0,
          total:
            innerRows * 50e-9 +
            (innerRows * avgRowSize) / (hw.storageBandwidthGbps * 1e9),
        },
        estimatedRows: innerRows,
        properties: { executions: formatRows(outerRows) },
        children: [],
      },
    ],
  };

  const semiJoinPlan: SimPlanNode = {
    operator: "Hash Semi Join",
    cost: semiJoinCost,
    estimatedRows: Math.round(outerRows * 0.3),
    properties: { strategy: "unnested semi join" },
    children: [
      {
        operator: "Seq Scan (customers)",
        cost: scanCost(hw, outerRows, avgRowSize),
        estimatedRows: outerRows,
        properties: { table: "customers" },
        children: [],
      },
      {
        operator: "Hash (orders)",
        cost: scanCost(hw, innerRows, avgRowSize),
        estimatedRows: innerRows,
        properties: { table: "orders" },
        children: [],
      },
    ],
  };

  const speedup =
    semiJoinCost.total > 0
      ? correlatedCost.total / semiJoinCost.total
      : 0;

  return (
    <DemoCard
      title="Subquery Unnesting (EXISTS to SEMI JOIN)"
      description="A correlated EXISTS subquery executes the inner query once per outer row (O(n*m)). The optimizer rewrites this to a Hash Semi Join that scans each table only once, achieving dramatic speedups."
    >
      <div class="demo-controls">
        <Slider
          label="Outer table (customers)"
          value={outerRows}
          min={1000}
          max={10_000_000}
          step={1000}
          format={formatRows}
          onChange={setOuterRows}
        />
        <Slider
          label="Inner table (orders)"
          value={innerRows}
          min={100}
          max={5_000_000}
          step={100}
          format={formatRows}
          onChange={setInnerRows}
        />
        <Select
          label="Hardware"
          value={hwCategory}
          options={HW_OPTIONS}
          onChange={setHwCategory}
        />
      </div>

      <div class="demo-metrics">
        <span class="demo-badge success">
          <span class="demo-badge-label">Speedup:</span>{" "}
          {speedup >= 1000
            ? `${(speedup / 1000).toFixed(1)}Kx`
            : `${speedup.toFixed(1)}x`}
        </span>
        <span class="demo-badge warning">
          <span class="demo-badge-label">Correlated:</span>{" "}
          {formatCost(correlatedCost.total)}
        </span>
        <span class="demo-badge success">
          <span class="demo-badge-label">Semi Join:</span>{" "}
          {formatCost(semiJoinCost.total)}
        </span>
      </div>

      <div class="demo-section">
        <h4>Original Query (Correlated Subquery)</h4>
        <pre class="demo-sql">
          {`SELECT * FROM customers c
WHERE EXISTS (
  SELECT 1 FROM orders o
  WHERE o.customer_id = c.id
    AND o.total > 100
)`}
        </pre>
      </div>

      <div class="demo-section">
        <h4>Rewritten Query (Semi Join)</h4>
        <pre class="demo-sql">
          {`SELECT c.* FROM customers c
SEMI JOIN orders o
  ON o.customer_id = c.id
  AND o.total > 100`}
        </pre>
      </div>

      <div class="demo-section">
        <h4>Cost Comparison</h4>
        <CostBarChart
          costs={[
            {
              label: "Correlated EXISTS",
              cost: correlatedCost,
            },
            {
              label: "Hash Semi Join [OPTIMIZED]",
              cost: semiJoinCost,
              highlight: true,
            },
          ]}
        />
      </div>

      <div class="demo-section">
        <h4>Plan Comparison</h4>
        <ComparisonView
          panels={[
            {
              title: "Before: Correlated Subquery",
              plan: correlatedPlan,
            },
            {
              title: "After: Semi Join",
              plan: semiJoinPlan,
              badge: "OPTIMIZED",
            },
          ]}
        />
      </div>
    </DemoCard>
  );
}

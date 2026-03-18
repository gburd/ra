/**
 * Demo 9: Distributed Query Planning
 *
 * Shows broadcast vs shuffle vs co-located join strategies for
 * distributed databases, and how cluster size and data distribution
 * affect the choice.
 */

import { useState } from "preact/hooks";
import type { DistributedJoinStrategy, HardwareCategory } from "src/components/demonstrations/types.ts";
import {
  chooseDistributedJoinStrategy,
  distributedJoinCost,
  formatCost,
  formatRows,
  getHardwareProfile,
} from "src/components/demonstrations/optimizer.ts";
import {
  CostBarChart,
  DemoCard,
  Slider,
  Toggle,
} from "src/components/demonstrations/DemoShared.tsx";

function strategyLabel(s: DistributedJoinStrategy): string {
  switch (s) {
    case "broadcast":
      return "Broadcast Join";
    case "shuffle":
      return "Shuffle (Repartition) Join";
    case "colocated":
      return "Co-located Join";
  }
}

export function Demo09Distributed() {
  const [leftRows, setLeftRows] = useState(100_000_000);
  const [rightRows, setRightRows] = useState(1_000_000);
  const [clusterNodes, setClusterNodes] = useState(8);
  const [isColocated, setIsColocated] = useState(false);

  const hw = getHardwareProfile("dual_socket_server");
  const avgRowSize = 100;

  const chosen = chooseDistributedJoinStrategy(
    leftRows,
    rightRows,
    clusterNodes,
    isColocated,
  );

  const strategies: DistributedJoinStrategy[] = [
    "broadcast",
    "shuffle",
    "colocated",
  ];

  const costs = strategies.map((strat) => {
    const cost = distributedJoinCost(
      hw,
      leftRows,
      rightRows,
      avgRowSize,
      clusterNodes,
      strat,
    );
    return {
      label: `${strategyLabel(strat)}${strat === chosen ? " [SELECTED]" : ""}`,
      cost,
      highlight: strat === chosen,
    };
  });

  const smallerRows = Math.min(leftRows, rightRows);
  const broadcastBytes = smallerRows * avgRowSize * (clusterNodes - 1);
  const shuffleBytes =
    (leftRows + rightRows) *
    avgRowSize *
    ((clusterNodes - 1) / clusterNodes);

  return (
    <DemoCard
      title="Distributed Query Planning"
      description="In a distributed database, join data must be co-located on the same node. The optimizer chooses between broadcasting the smaller table, shuffling (repartitioning) both tables, or executing locally if data is already co-located."
    >
      <div class="demo-controls">
        <Slider
          label="Left table (fact table)"
          value={leftRows}
          min={100000}
          max={10_000_000_000}
          step={100000}
          format={formatRows}
          onChange={setLeftRows}
        />
        <Slider
          label="Right table (dimension table)"
          value={rightRows}
          min={1000}
          max={1_000_000_000}
          step={1000}
          format={formatRows}
          onChange={setRightRows}
        />
        <Slider
          label="Cluster nodes"
          value={clusterNodes}
          min={2}
          max={128}
          format={(v) => `${v} nodes`}
          onChange={setClusterNodes}
        />
        <Toggle
          label="Data co-located (same partition key)"
          checked={isColocated}
          onChange={setIsColocated}
        />
      </div>

      <div class="demo-metrics">
        <span class="demo-badge success">
          <span class="demo-badge-label">Selected:</span>{" "}
          {strategyLabel(chosen)}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Broadcast Data:</span>{" "}
          {(broadcastBytes / 1e9).toFixed(1)} GB
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Shuffle Data:</span>{" "}
          {(shuffleBytes / 1e9).toFixed(1)} GB
        </span>
      </div>

      <div class="demo-section">
        <h4>SQL Query</h4>
        <pre class="demo-sql">
          {`-- Distributed across ${clusterNodes} nodes
SELECT f.*, d.name, d.category
FROM fact_table f
JOIN dimension_table d ON f.dim_id = d.id
WHERE f.date >= '2024-01-01'`}
        </pre>
      </div>

      <div class="demo-section">
        <h4>Strategy Cost Comparison</h4>
        <CostBarChart costs={costs} />
      </div>

      <div class="demo-section">
        <h4>Network Transfer Comparison</h4>
        <div class="demo-table-scroll">
          <table class="demo-table">
            <thead>
              <tr>
                <th>Strategy</th>
                <th>Data Transferred</th>
                <th>Transfers per Node</th>
                <th>Network Time</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Broadcast</td>
                <td class="mono">
                  {(broadcastBytes / 1e9).toFixed(1)} GB
                </td>
                <td class="mono">{clusterNodes - 1}</td>
                <td class="mono">
                  {formatCost(broadcastBytes / (1.25 * 1e9))}
                </td>
              </tr>
              <tr>
                <td>Shuffle</td>
                <td class="mono">
                  {(shuffleBytes / 1e9).toFixed(1)} GB
                </td>
                <td class="mono">{clusterNodes - 1} (per node)</td>
                <td class="mono">
                  {formatCost(shuffleBytes / (1.25 * 1e9))}
                </td>
              </tr>
              <tr>
                <td>Co-located</td>
                <td class="mono">0 GB</td>
                <td class="mono">0</td>
                <td class="mono">0ms</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="demo-section">
        <h4>Selection Rules</h4>
        <div class="demo-rules">
          <div class={`demo-rule ${chosen === "colocated" ? "active" : ""}`}>
            <strong>Co-located:</strong> Data is partitioned on the join
            key. Zero network transfer; each node joins its local
            partition.
          </div>
          <div class={`demo-rule ${chosen === "broadcast" ? "active" : ""}`}>
            <strong>Broadcast:</strong> Smaller table has fewer than{" "}
            {formatRows(100000 * clusterNodes)} rows. Send it to every
            node. Network cost: small table * (N-1) copies.
          </div>
          <div class={`demo-rule ${chosen === "shuffle" ? "active" : ""}`}>
            <strong>Shuffle:</strong> Both tables are large. Repartition
            both on join key. Network cost: ~(1-1/N) of total data.
          </div>
        </div>
      </div>
    </DemoCard>
  );
}

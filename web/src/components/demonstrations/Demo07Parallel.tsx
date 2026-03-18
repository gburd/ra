/**
 * Demo 7: Parallel Query Execution
 *
 * Shows serial vs parallel execution plans and how data size,
 * CPU cores, and NUMA topology affect parallelism decisions.
 */

import { useState } from "preact/hooks";
import type { HardwareCategory } from "src/components/demonstrations/types.ts";
import {
  formatCost,
  formatRows,
  getHardwareProfile,
  hashJoinCost,
  parallelHashJoinCost,
  parallelScanCost,
  scanCost,
} from "src/components/demonstrations/optimizer.ts";
import {
  CostBarChart,
  DemoCard,
  Select,
  Slider,
} from "src/components/demonstrations/DemoShared.tsx";

const HW_OPTIONS: readonly {
  readonly value: HardwareCategory;
  readonly label: string;
}[] = [
  { value: "raspberry_pi", label: "Raspberry Pi 4 (4 cores)" },
  { value: "desktop_budget", label: "Desktop Budget (12 cores)" },
  { value: "desktop_workstation", label: "Desktop Workstation (24 cores)" },
  { value: "entry_server", label: "Entry Server (40 cores)" },
  { value: "dual_socket_server", label: "Dual-Socket (128 cores)" },
  { value: "data_warehouse", label: "Data Warehouse (256 cores)" },
];

export function Demo07Parallel() {
  const [rowCount, setRowCount] = useState(50_000_000);
  const [hwCategory, setHwCategory] =
    useState<HardwareCategory>("dual_socket_server");
  const [parallelism, setParallelism] = useState(8);

  const hw = getHardwareProfile(hwCategory);
  const maxParallelism = Math.min(hw.cpuCores, 256);
  const effectiveParallelism = Math.min(parallelism, maxParallelism);
  const avgRowSize = 100;

  const seqScan = scanCost(hw, rowCount, avgRowSize);
  const parScan = parallelScanCost(
    hw,
    rowCount,
    avgRowSize,
    effectiveParallelism,
  );

  const buildRows = rowCount / 10;
  const seqJoin = hashJoinCost(hw, buildRows, rowCount, avgRowSize);
  const parJoin = parallelHashJoinCost(
    hw,
    buildRows,
    rowCount,
    avgRowSize,
    effectiveParallelism,
  );

  const scanSpeedup =
    parScan.total > 0 ? seqScan.total / parScan.total : 1;
  const joinSpeedup =
    parJoin.total > 0 ? seqJoin.total / parJoin.total : 1;

  // Compute scaling efficiency at various parallelism levels
  const scalingPoints = [1, 2, 4, 8, 16, 32, 64, 128, 256]
    .filter((p) => p <= maxParallelism)
    .map((p) => {
      const par = parallelScanCost(hw, rowCount, avgRowSize, p);
      const speedup = seqScan.total / par.total;
      const efficiency = (speedup / p) * 100;
      return { parallelism: p, speedup, efficiency };
    });

  return (
    <DemoCard
      title="Parallel Query Execution"
      description="Compare serial vs parallel execution. Parallelism introduces coordination overhead that limits scaling. NUMA topology adds cross-socket penalties for high parallelism degrees."
    >
      <div class="demo-controls">
        <Slider
          label="Table rows"
          value={rowCount}
          min={100000}
          max={1_000_000_000}
          step={100000}
          format={formatRows}
          onChange={setRowCount}
        />
        <Select
          label="Hardware"
          value={hwCategory}
          options={HW_OPTIONS}
          onChange={setHwCategory}
        />
        <Slider
          label="Parallel workers"
          value={parallelism}
          min={1}
          max={Math.min(maxParallelism, 256)}
          format={(v) =>
            `${v} worker${v > 1 ? "s" : ""} (max ${maxParallelism})`
          }
          onChange={setParallelism}
        />
      </div>

      <div class="demo-metrics">
        <span class="demo-badge success">
          <span class="demo-badge-label">Scan Speedup:</span>{" "}
          {scanSpeedup.toFixed(1)}x
        </span>
        <span class="demo-badge success">
          <span class="demo-badge-label">Join Speedup:</span>{" "}
          {joinSpeedup.toFixed(1)}x
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">NUMA Nodes:</span>{" "}
          {hw.numaNodes}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Total Cores:</span>{" "}
          {hw.cpuCores}
        </span>
      </div>

      <div class="demo-section">
        <h4>Scan Cost: Serial vs Parallel</h4>
        <CostBarChart
          costs={[
            { label: "Serial Scan", cost: seqScan },
            {
              label: `Parallel Scan (${effectiveParallelism} workers)`,
              cost: parScan,
              highlight: true,
            },
          ]}
        />
      </div>

      <div class="demo-section">
        <h4>Hash Join: Serial vs Parallel</h4>
        <CostBarChart
          costs={[
            { label: "Serial Hash Join", cost: seqJoin },
            {
              label: `Parallel Hash Join (${effectiveParallelism} workers)`,
              cost: parJoin,
              highlight: true,
            },
          ]}
        />
      </div>

      <div class="demo-section">
        <h4>Scaling Efficiency</h4>
        <div class="demo-table-scroll">
          <table class="demo-table">
            <thead>
              <tr>
                <th>Workers</th>
                <th>Speedup</th>
                <th>Efficiency</th>
                <th>Scaling</th>
              </tr>
            </thead>
            <tbody>
              {scalingPoints.map((pt) => (
                <tr key={pt.parallelism}>
                  <td>{pt.parallelism}</td>
                  <td class="mono">{pt.speedup.toFixed(1)}x</td>
                  <td class="mono">{pt.efficiency.toFixed(0)}%</td>
                  <td>
                    <div class="demo-mini-bar">
                      <div
                        class="demo-mini-bar-fill"
                        style={{
                          width: `${Math.min(pt.efficiency, 100)}%`,
                        }}
                      />
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div class="demo-section">
        <h4>Key Insight</h4>
        <div class="demo-insight">
          Coordination overhead (5-8% per additional worker) prevents
          linear scaling. On NUMA systems with {hw.numaNodes} nodes,
          cross-socket memory access adds latency when parallelism
          exceeds the per-socket core count. The optimal parallelism
          degree balances throughput gain against coordination cost.
        </div>
      </div>
    </DemoCard>
  );
}

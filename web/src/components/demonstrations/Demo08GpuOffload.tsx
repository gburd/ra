/**
 * Demo 8: GPU Offloading Decision
 *
 * Shows when it's beneficial to offload computation to the GPU
 * vs keeping it on CPU, accounting for PCIe transfer overhead.
 */

import { useState } from "preact/hooks";
import type { HardwareCategory } from "src/components/demonstrations/types.ts";
import {
  chooseDevicePlacement,
  formatCost,
  formatRows,
  getHardwareProfile,
  gpuAggregationCost,
  gpuHashJoinCost,
  gpuScanCost,
  hashAggCost,
  hashJoinCost,
  scanCost,
} from "src/components/demonstrations/optimizer.ts";
import {
  CostBarChart,
  DemoCard,
  Select,
  Slider,
} from "src/components/demonstrations/DemoShared.tsx";

const GPU_HW_OPTIONS: readonly {
  readonly value: HardwareCategory;
  readonly label: string;
}[] = [
  { value: "desktop_workstation", label: "RTX 4070 (12GB, 46 SMs)" },
  { value: "gpu_server_a100", label: "A100 80GB (108 SMs)" },
  { value: "gpu_server_h100", label: "H100 80GB (132 SMs)" },
  { value: "olap_database", label: "OLAP + A100 (108 SMs)" },
];

type Operation = "scan" | "hash_join" | "aggregation";

const OP_OPTIONS: readonly {
  readonly value: Operation;
  readonly label: string;
}[] = [
  { value: "scan", label: "Full Table Scan" },
  { value: "hash_join", label: "Hash Join" },
  { value: "aggregation", label: "Aggregation (GROUP BY)" },
];

export function Demo08GpuOffload() {
  const [rowCount, setRowCount] = useState(100_000_000);
  const [hwCategory, setHwCategory] =
    useState<HardwareCategory>("gpu_server_a100");
  const [operation, setOperation] = useState<Operation>("hash_join");

  const hw = getHardwareProfile(hwCategory);
  const avgRowSize = 100;

  let cpuCostVal, gpuCostVal;
  switch (operation) {
    case "scan":
      cpuCostVal = scanCost(hw, rowCount, avgRowSize);
      gpuCostVal = gpuScanCost(hw, rowCount, avgRowSize);
      break;
    case "hash_join": {
      const buildRows = rowCount / 10;
      cpuCostVal = hashJoinCost(hw, buildRows, rowCount, avgRowSize);
      gpuCostVal = gpuHashJoinCost(hw, buildRows, rowCount, avgRowSize);
      break;
    }
    case "aggregation": {
      const groups = Math.min(rowCount / 100, 1_000_000);
      cpuCostVal = hashAggCost(hw, rowCount, groups, avgRowSize);
      gpuCostVal = gpuAggregationCost(hw, rowCount, groups, avgRowSize);
      break;
    }
  }

  const placement = chooseDevicePlacement(hw, cpuCostVal, gpuCostVal);

  const transferBytes = rowCount * avgRowSize;
  const transferTime = hw.pcieBandwidthGbps > 0
    ? transferBytes / (hw.pcieBandwidthGbps * 1e9)
    : 0;

  const dataFitsGpu = transferBytes <= hw.gpuMemoryGb * 1e9;

  return (
    <DemoCard
      title="GPU Offloading Decision"
      description="GPU acceleration provides massive parallelism but requires transferring data over PCIe. For bandwidth-bound scans, the PCIe bottleneck often makes CPU faster. For compute-heavy joins and aggregations, GPU wins on large data."
    >
      <div class="demo-controls">
        <Slider
          label="Row count"
          value={rowCount}
          min={10000}
          max={1_000_000_000}
          step={10000}
          format={formatRows}
          onChange={setRowCount}
        />
        <Select
          label="GPU Hardware"
          value={hwCategory}
          options={GPU_HW_OPTIONS}
          onChange={setHwCategory}
        />
        <Select
          label="Operation"
          value={operation}
          options={OP_OPTIONS}
          onChange={setOperation}
        />
      </div>

      <div class="demo-metrics">
        <span
          class={`demo-badge ${
            placement === "gpu"
              ? "success"
              : placement === "hybrid"
                ? "warning"
                : "default"
          }`}
        >
          <span class="demo-badge-label">Placement:</span>{" "}
          {placement.toUpperCase()}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Data Size:</span>{" "}
          {(transferBytes / 1e9).toFixed(1)} GB
        </span>
        <span class={`demo-badge ${dataFitsGpu ? "success" : "error"}`}>
          <span class="demo-badge-label">Fits GPU Memory:</span>{" "}
          {dataFitsGpu ? "Yes" : "No"} ({hw.gpuMemoryGb} GB)
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">PCIe Transfer:</span>{" "}
          {formatCost(transferTime)}
        </span>
      </div>

      <div class="demo-section">
        <h4>CPU vs GPU Cost</h4>
        <CostBarChart
          costs={[
            {
              label: "CPU Execution",
              cost: cpuCostVal,
              highlight: placement === "cpu",
            },
            {
              label: "GPU Execution (incl. PCIe transfer)",
              cost: gpuCostVal,
              highlight: placement === "gpu",
            },
          ]}
        />
      </div>

      <div class="demo-section">
        <h4>GPU Cost Breakdown</h4>
        <div class="demo-table-scroll">
          <table class="demo-table">
            <thead>
              <tr>
                <th>Component</th>
                <th>Time</th>
                <th>% of Total</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>GPU Compute</td>
                <td class="mono">{formatCost(gpuCostVal.cpu)}</td>
                <td class="mono">
                  {gpuCostVal.total > 0
                    ? `${((gpuCostVal.cpu / gpuCostVal.total) * 100).toFixed(1)}%`
                    : "N/A"}
                </td>
              </tr>
              <tr>
                <td>PCIe Transfer (Host to Device)</td>
                <td class="mono">{formatCost(gpuCostVal.network)}</td>
                <td class="mono">
                  {gpuCostVal.total > 0
                    ? `${((gpuCostVal.network / gpuCostVal.total) * 100).toFixed(1)}%`
                    : "N/A"}
                </td>
              </tr>
              <tr>
                <td>I/O (Storage)</td>
                <td class="mono">{formatCost(gpuCostVal.io)}</td>
                <td class="mono">
                  {gpuCostVal.total > 0
                    ? `${((gpuCostVal.io / gpuCostVal.total) * 100).toFixed(1)}%`
                    : "N/A"}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="demo-section">
        <h4>Key Insight</h4>
        <div class="demo-insight">
          For pure scans, CPU usually wins because PCIe bandwidth
          ({hw.pcieBandwidthGbps} GB/s) is less than CPU memory
          bandwidth ({hw.cpuMemoryBandwidthGbps} GB/s). GPU excels at
          compute-intensive operations like hash joins and aggregations
          where {hw.gpuSmCount} streaming multiprocessors provide
          massive parallelism. The crossover point depends on the
          compute-to-data ratio of the operation.
        </div>
      </div>
    </DemoCard>
  );
}

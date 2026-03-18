/**
 * Demo 4: Aggregation Strategy Selection
 *
 * Shows how the optimizer picks between Hash Aggregation, Sort
 * Aggregation, Streaming Aggregation, and Two-Phase Aggregation
 * based on group cardinality, available memory, and CPU cores.
 */

import { useState } from "preact/hooks";
import type { AggregationStrategy, HardwareCategory } from "src/components/demonstrations/types.ts";
import {
  chooseBestAggregation,
  formatCost,
  formatRows,
  getHardwareProfile,
  hashAggCost,
  sortAggCost,
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
  { value: "raspberry_pi", label: "Raspberry Pi 4 (4GB)" },
  { value: "desktop_budget", label: "Desktop Budget (16GB)" },
  { value: "entry_server", label: "Entry Server (128GB)" },
  { value: "dual_socket_server", label: "Dual-Socket Server (512GB)" },
  { value: "data_warehouse", label: "Data Warehouse (2TB)" },
];

function strategyLabel(s: AggregationStrategy): string {
  switch (s) {
    case "hash_agg":
      return "Hash Aggregation";
    case "sort_agg":
      return "Sort Aggregation";
    case "streaming_agg":
      return "Streaming Aggregation";
    case "two_phase_agg":
      return "Two-Phase Aggregation";
  }
}

export function Demo04Aggregation() {
  const [inputRows, setInputRows] = useState(10_000_000);
  const [groupCount, setGroupCount] = useState(100_000);
  const [hwCategory, setHwCategory] =
    useState<HardwareCategory>("entry_server");
  const [memoryPct, setMemoryPct] = useState(50);

  const hw = getHardwareProfile(hwCategory);
  const avgRowSize = 100;
  const availMem = hw.memoryGb * 1e9 * (memoryPct / 100);

  const chosen = chooseBestAggregation(
    hw,
    inputRows,
    groupCount,
    avgRowSize,
    availMem,
  );

  const strategies: AggregationStrategy[] = [
    "hash_agg",
    "sort_agg",
    "streaming_agg",
    "two_phase_agg",
  ];

  const costs = strategies.map((strat) => {
    let cost;
    switch (strat) {
      case "hash_agg":
        cost = hashAggCost(hw, inputRows, groupCount, avgRowSize);
        break;
      case "sort_agg":
        cost = sortAggCost(hw, inputRows, avgRowSize);
        break;
      case "streaming_agg":
        cost = hashAggCost(hw, inputRows, 1, avgRowSize);
        break;
      case "two_phase_agg": {
        const partitions = hw.cpuCores;
        const perPartition = hashAggCost(
          hw,
          inputRows / partitions,
          Math.min(groupCount, inputRows / partitions),
          avgRowSize,
        );
        const mergeCost = hashAggCost(
          hw,
          groupCount * partitions,
          groupCount,
          avgRowSize,
        );
        cost = {
          cpu: perPartition.cpu + mergeCost.cpu,
          io: perPartition.io + mergeCost.io,
          memory: perPartition.memory * partitions + mergeCost.memory,
          network: 0,
          total:
            perPartition.cpu +
            mergeCost.cpu +
            perPartition.io +
            mergeCost.io +
            perPartition.memory * partitions +
            mergeCost.memory,
        };
        break;
      }
    }

    return {
      label: `${strategyLabel(strat)}${strat === chosen ? " [SELECTED]" : ""}`,
      cost,
      highlight: strat === chosen,
    };
  });

  const htSize = groupCount * 64;
  const htFits = htSize <= availMem;

  return (
    <DemoCard
      title="Aggregation Strategy Selection"
      description="Explore how group cardinality, input size, and available memory determine whether the optimizer uses hash aggregation, sort-based grouping, streaming (for single-group), or two-phase parallel aggregation."
    >
      <div class="demo-controls">
        <Slider
          label="Input rows"
          value={inputRows}
          min={1000}
          max={1_000_000_000}
          step={1000}
          format={formatRows}
          onChange={setInputRows}
        />
        <Slider
          label="Distinct groups"
          value={groupCount}
          min={1}
          max={100_000_000}
          step={1}
          format={formatRows}
          onChange={setGroupCount}
        />
        <Select
          label="Hardware"
          value={hwCategory}
          options={HW_OPTIONS}
          onChange={setHwCategory}
        />
        <Slider
          label="Available memory"
          value={memoryPct}
          min={1}
          max={80}
          format={(v) =>
            `${v}% (${((hw.memoryGb * v) / 100).toFixed(1)} GB)`
          }
          onChange={setMemoryPct}
        />
      </div>

      <div class="demo-metrics">
        <span class="demo-badge success">
          <span class="demo-badge-label">Selected:</span>{" "}
          {strategyLabel(chosen)}
        </span>
        <span class={`demo-badge ${htFits ? "success" : "warning"}`}>
          <span class="demo-badge-label">Hash Table:</span>{" "}
          {(htSize / 1e6).toFixed(1)} MB {htFits ? "(fits)" : "(spills)"}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Group Ratio:</span>{" "}
          {((groupCount / inputRows) * 100).toFixed(2)}%
        </span>
      </div>

      <div class="demo-section">
        <h4>SQL Query</h4>
        <pre class="demo-sql">
          {`SELECT region, COUNT(*), AVG(amount)
FROM transactions
GROUP BY region`}
        </pre>
      </div>

      <div class="demo-section">
        <h4>Strategy Cost Comparison</h4>
        <CostBarChart costs={costs} />
      </div>

      <div class="demo-section">
        <h4>Selection Rules</h4>
        <div class="demo-rules">
          <div class={`demo-rule ${chosen === "streaming_agg" ? "active" : ""}`}>
            <strong>Streaming:</strong> Only 1 group (e.g., COUNT(*) with
            no GROUP BY). Single pass, O(1) memory.
          </div>
          <div class={`demo-rule ${chosen === "hash_agg" ? "active" : ""}`}>
            <strong>Hash Agg:</strong> Hash table fits in memory and groups
            are fewer than 50% of input rows. O(n) time, O(groups) memory.
          </div>
          <div class={`demo-rule ${chosen === "sort_agg" ? "active" : ""}`}>
            <strong>Sort Agg:</strong> When hash table would exceed memory.
            Sort then scan: O(n log n) time but no hash table needed.
          </div>
          <div class={`demo-rule ${chosen === "two_phase_agg" ? "active" : ""}`}>
            <strong>Two-Phase:</strong> Massive groups ({">"}1M), many cores
            ({">"}=16), and hash table won't fit. Partial agg per partition,
            then merge.
          </div>
        </div>
      </div>
    </DemoCard>
  );
}

/**
 * Demo 3: Join Algorithm Selection
 *
 * Interactive demonstration of how the optimizer selects between
 * Nested Loop, Hash Join, Sort-Merge Join, and Index Nested Loop
 * based on table sizes, available memory, and index presence.
 */

import { useState } from "preact/hooks";
import type { HardwareCategory, JoinAlgorithm } from "src/components/demonstrations/types.ts";
import {
  chooseBestJoin,
  formatCost,
  formatRows,
  getHardwareProfile,
  hashJoinCost,
  nestedLoopJoinCost,
  sortMergeJoinCost,
} from "src/components/demonstrations/optimizer.ts";
import {
  CostBarChart,
  DemoCard,
  Select,
  Slider,
  Toggle,
} from "src/components/demonstrations/DemoShared.tsx";

const HW_OPTIONS: readonly {
  readonly value: HardwareCategory;
  readonly label: string;
}[] = [
  { value: "raspberry_pi", label: "Raspberry Pi 4 (4GB)" },
  { value: "desktop_budget", label: "Desktop Budget (16GB)" },
  { value: "desktop_workstation", label: "Desktop Workstation (64GB)" },
  { value: "entry_server", label: "Entry Server (128GB)" },
  { value: "dual_socket_server", label: "Dual-Socket Server (512GB)" },
  { value: "data_warehouse", label: "Data Warehouse (2TB)" },
];

export function Demo03JoinSelection() {
  const [leftRows, setLeftRows] = useState(500_000);
  const [rightRows, setRightRows] = useState(10_000);
  const [hwCategory, setHwCategory] =
    useState<HardwareCategory>("dual_socket_server");
  const [hasIndex, setHasIndex] = useState(false);
  const [memoryPct, setMemoryPct] = useState(50);

  const hw = getHardwareProfile(hwCategory);
  const avgRowSize = 100;
  const availMem = hw.memoryGb * 1e9 * (memoryPct / 100);

  const chosen = chooseBestJoin(
    hw,
    leftRows,
    rightRows,
    avgRowSize,
    availMem,
    hasIndex,
  );

  const algorithms: JoinAlgorithm[] = [
    "nested_loop",
    "hash_join",
    "sort_merge",
    "index_nested_loop",
  ];

  const costs = algorithms.map((alg) => {
    let cost;
    switch (alg) {
      case "nested_loop":
        cost = nestedLoopJoinCost(hw, leftRows, rightRows, avgRowSize);
        break;
      case "hash_join":
        cost = hashJoinCost(
          hw,
          Math.min(leftRows, rightRows),
          Math.max(leftRows, rightRows),
          avgRowSize,
        );
        break;
      case "sort_merge":
        cost = sortMergeJoinCost(hw, leftRows, rightRows, avgRowSize);
        break;
      case "index_nested_loop":
        cost = hasIndex
          ? nestedLoopJoinCost(
              hw,
              Math.min(leftRows, rightRows),
              100,
              avgRowSize,
            )
          : nestedLoopJoinCost(hw, leftRows, rightRows, avgRowSize);
        break;
    }

    return {
      label: `${alg.replace(/_/g, " ")}${alg === chosen ? " [SELECTED]" : ""}`,
      cost,
      highlight: alg === chosen,
    };
  });

  const htSize = Math.min(leftRows, rightRows) * avgRowSize * 2;
  const htFits = htSize <= availMem;

  return (
    <DemoCard
      title="Join Algorithm Selection"
      description="Adjust table sizes, hardware, and memory to see how the optimizer chooses between four join algorithms. The selection depends on relative table sizes, available memory for hash tables, and index availability."
    >
      <div class="demo-controls">
        <Slider
          label="Left table rows"
          value={leftRows}
          min={10}
          max={50_000_000}
          step={10}
          format={formatRows}
          onChange={setLeftRows}
        />
        <Slider
          label="Right table rows"
          value={rightRows}
          min={10}
          max={50_000_000}
          step={10}
          format={formatRows}
          onChange={setRightRows}
        />
        <Select
          label="Hardware"
          value={hwCategory}
          options={HW_OPTIONS}
          onChange={setHwCategory}
        />
        <Slider
          label="Available memory for query"
          value={memoryPct}
          min={1}
          max={80}
          format={(v) =>
            `${v}% (${((hw.memoryGb * v) / 100).toFixed(1)} GB)`
          }
          onChange={setMemoryPct}
        />
        <Toggle
          label="Index on join column"
          checked={hasIndex}
          onChange={setHasIndex}
        />
      </div>

      <div class="demo-metrics">
        <span class={`demo-badge ${chosen === "hash_join" ? "success" : "default"}`}>
          <span class="demo-badge-label">Selected:</span>{" "}
          {chosen.replace(/_/g, " ")}
        </span>
        <span class={`demo-badge ${htFits ? "success" : "warning"}`}>
          <span class="demo-badge-label">Hash Table:</span>{" "}
          {(htSize / 1e9).toFixed(2)} GB {htFits ? "(fits)" : "(spills)"}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Memory Budget:</span>{" "}
          {(availMem / 1e9).toFixed(1)} GB
        </span>
      </div>

      <div class="demo-section">
        <h4>Algorithm Cost Comparison</h4>
        <CostBarChart costs={costs} />
      </div>

      <div class="demo-section">
        <h4>Selection Rules</h4>
        <div class="demo-rules">
          <div class={`demo-rule ${chosen === "index_nested_loop" ? "active" : ""}`}>
            <strong>Index Nested Loop:</strong> When an index exists
            and the smaller table has fewer than 1,000 rows.
          </div>
          <div class={`demo-rule ${chosen === "nested_loop" ? "active" : ""}`}>
            <strong>Nested Loop:</strong> When both tables are small
            (outer &lt; 100, inner &lt; 10,000). O(n*m) cost dominates.
          </div>
          <div class={`demo-rule ${chosen === "hash_join" ? "active" : ""}`}>
            <strong>Hash Join:</strong> When the hash table for the
            smaller side fits in available memory. Build O(n), probe O(m).
          </div>
          <div class={`demo-rule ${chosen === "sort_merge" ? "active" : ""}`}>
            <strong>Sort-Merge Join:</strong> Fallback when hash table
            won't fit. Sort both sides O(n log n + m log m), then merge.
          </div>
        </div>
      </div>
    </DemoCard>
  );
}

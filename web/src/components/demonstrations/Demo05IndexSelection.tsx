/**
 * Demo 5: Index Selection
 *
 * Shows how selectivity determines whether the optimizer uses
 * sequential scan, index scan, bitmap scan, or index-only scan.
 */

import { useState } from "preact/hooks";
import type { HardwareCategory, ScanMethod } from "src/components/demonstrations/types.ts";
import {
  chooseScanMethod,
  formatCost,
  formatRows,
  getHardwareProfile,
  indexScanCost,
  scanCost,
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
  { value: "raspberry_pi", label: "Raspberry Pi 4 (microSD)" },
  { value: "entry_server", label: "Entry Server (SATA SSD)" },
  { value: "oltp_database", label: "OLTP Database (NVMe Optane)" },
  { value: "data_warehouse", label: "Data Warehouse (NVMe Array)" },
];

function scanMethodLabel(m: ScanMethod): string {
  switch (m) {
    case "sequential_scan":
      return "Sequential Scan";
    case "index_scan":
      return "Index Scan";
    case "bitmap_scan":
      return "Bitmap Scan";
    case "index_only_scan":
      return "Index-Only Scan";
  }
}

export function Demo05IndexSelection() {
  const [totalRows, setTotalRows] = useState(10_000_000);
  const [selectivityPct, setSelectivityPct] = useState(5);
  const [hwCategory, setHwCategory] =
    useState<HardwareCategory>("oltp_database");
  const [hasIndex, setHasIndex] = useState(true);
  const [hasMultipleIndexes, setHasMultipleIndexes] = useState(false);

  const hw = getHardwareProfile(hwCategory);
  const avgRowSize = 100;
  const selectivity = selectivityPct / 100;
  const selectedRows = Math.round(totalRows * selectivity);

  const chosen = chooseScanMethod(
    hw,
    totalRows,
    selectivity,
    hasIndex,
    hasMultipleIndexes,
  );

  const methods: ScanMethod[] = [
    "sequential_scan",
    "index_scan",
    "bitmap_scan",
    "index_only_scan",
  ];

  const costs = methods.map((method) => {
    let cost;
    switch (method) {
      case "sequential_scan":
        cost = scanCost(hw, totalRows, avgRowSize);
        break;
      case "index_scan":
        cost = hasIndex
          ? indexScanCost(hw, totalRows, selectedRows, avgRowSize)
          : scanCost(hw, totalRows, avgRowSize);
        break;
      case "bitmap_scan": {
        const idxCost = hasIndex
          ? indexScanCost(hw, totalRows, selectedRows, avgRowSize)
          : scanCost(hw, totalRows, avgRowSize);
        cost = {
          ...idxCost,
          io: idxCost.io * 0.7,
          total: idxCost.cpu + idxCost.io * 0.7 + idxCost.memory + idxCost.network,
        };
        break;
      }
      case "index_only_scan":
        cost = hasIndex
          ? indexScanCost(
              hw,
              totalRows,
              selectedRows,
              20,
            )
          : scanCost(hw, totalRows, avgRowSize);
        break;
    }

    return {
      label: `${scanMethodLabel(method)}${method === chosen ? " [SELECTED]" : ""}`,
      cost,
      highlight: method === chosen,
    };
  });

  return (
    <DemoCard
      title="Index Selection"
      description="Selectivity is the key factor: low selectivity (few rows match) favors index scans, while high selectivity (many rows match) favors sequential scans. Storage hardware determines the crossover point."
    >
      <div class="demo-controls">
        <Slider
          label="Total table rows"
          value={totalRows}
          min={10000}
          max={100_000_000}
          step={10000}
          format={formatRows}
          onChange={setTotalRows}
        />
        <Slider
          label="Selectivity (% rows matching predicate)"
          value={selectivityPct}
          min={0}
          max={100}
          step={1}
          format={(v) => `${v}% (${formatRows(Math.round(totalRows * v / 100))} rows)`}
          onChange={setSelectivityPct}
        />
        <Select
          label="Hardware"
          value={hwCategory}
          options={HW_OPTIONS}
          onChange={setHwCategory}
        />
        <Toggle
          label="Index on predicate column"
          checked={hasIndex}
          onChange={setHasIndex}
        />
        <Toggle
          label="Multiple indexes available"
          checked={hasMultipleIndexes}
          onChange={setHasMultipleIndexes}
        />
      </div>

      <div class="demo-metrics">
        <span class="demo-badge success">
          <span class="demo-badge-label">Selected:</span>{" "}
          {scanMethodLabel(chosen)}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Rows Selected:</span>{" "}
          {formatRows(selectedRows)} of {formatRows(totalRows)}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Storage:</span>{" "}
          {hw.storageType}
        </span>
      </div>

      <div class="demo-section">
        <h4>SQL Query</h4>
        <pre class="demo-sql">
          {`SELECT * FROM events
WHERE created_at > '2024-01-01'
  AND status = 'active'`}
        </pre>
      </div>

      <div class="demo-section">
        <h4>Scan Method Cost Comparison</h4>
        <CostBarChart costs={costs} />
      </div>

      <div class="demo-section">
        <h4>Selection Thresholds</h4>
        <div class="demo-rules">
          <div class={`demo-rule ${chosen === "index_only_scan" ? "active" : ""}`}>
            <strong>Index-Only Scan:</strong> Selectivity &lt; 1% and
            all needed columns are in the index (covering index).
            No heap access required.
          </div>
          <div class={`demo-rule ${chosen === "index_scan" ? "active" : ""}`}>
            <strong>Index Scan:</strong> Selectivity 1-5%. Random I/O to
            fetch heap tuples, but reads far fewer pages than seq scan.
          </div>
          <div class={`demo-rule ${chosen === "bitmap_scan" ? "active" : ""}`}>
            <strong>Bitmap Scan:</strong> Selectivity 5-20% with
            multiple indexes. Builds a bitmap of matching pages, then
            does sequential heap reads (reducing random I/O).
          </div>
          <div class={`demo-rule ${chosen === "sequential_scan" ? "active" : ""}`}>
            <strong>Sequential Scan:</strong> Selectivity &gt; 20% or
            no index. Sequential I/O is faster than random when reading
            most of the table.
          </div>
        </div>
      </div>
    </DemoCard>
  );
}

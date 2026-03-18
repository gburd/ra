/**
 * Demo 2: Hardware-Specific Plans
 *
 * Same query on different hardware profiles shows how the optimizer
 * adapts algorithm choices based on CPU cores, memory, storage type,
 * and GPU availability.
 */

import { useState } from "preact/hooks";
import type { HardwareCategory } from "src/components/demonstrations/types.ts";
import {
  chooseBestJoin,
  formatCost,
  formatRows,
  getHardwareProfile,
  getAllHardwareCategories,
  hashJoinCost,
  nestedLoopJoinCost,
  scanCost,
  sortMergeJoinCost,
} from "src/components/demonstrations/optimizer.ts";
import {
  CostBarChart,
  DemoCard,
  MetricRow,
  Select,
  Slider,
} from "src/components/demonstrations/DemoShared.tsx";

const HW_OPTIONS: readonly {
  readonly value: HardwareCategory;
  readonly label: string;
}[] = getAllHardwareCategories().map((cat) => ({
  value: cat,
  label: getHardwareProfile(cat).name,
}));

export function Demo02Hardware() {
  const [rowCount, setRowCount] = useState(5_000_000);
  const avgRowSize = 100;

  const results = getAllHardwareCategories().map((cat) => {
    const hw = getHardwareProfile(cat);
    const joinAlg = chooseBestJoin(
      hw,
      rowCount,
      rowCount / 10,
      avgRowSize,
      hw.memoryGb * 1e9 * 0.5,
      false,
    );

    const seqCost = scanCost(hw, rowCount, avgRowSize);

    let joinCostVal;
    switch (joinAlg) {
      case "hash_join":
        joinCostVal = hashJoinCost(
          hw,
          rowCount / 10,
          rowCount,
          avgRowSize,
        );
        break;
      case "sort_merge":
        joinCostVal = sortMergeJoinCost(
          hw,
          rowCount,
          rowCount / 10,
          avgRowSize,
        );
        break;
      default:
        joinCostVal = nestedLoopJoinCost(
          hw,
          rowCount,
          rowCount / 10,
          avgRowSize,
        );
    }

    return {
      category: cat,
      hw,
      joinAlg,
      scanCost: seqCost,
      joinCost: joinCostVal,
    };
  });

  const scanCosts = results.map((r) => ({
    label: r.hw.name,
    cost: r.scanCost,
  }));

  const joinCosts = results.map((r) => ({
    label: `${r.hw.name} (${r.joinAlg.replace(/_/g, " ")})`,
    cost: r.joinCost,
  }));

  return (
    <DemoCard
      title="Hardware-Specific Query Plans"
      description="The same query produces different execution plans depending on hardware. Storage type affects scan costs, memory determines hash table feasibility, and CPU cores influence parallelism decisions."
    >
      <div class="demo-controls">
        <Slider
          label="Table rows"
          value={rowCount}
          min={10000}
          max={100_000_000}
          step={10000}
          format={(v) => formatRows(v)}
          onChange={setRowCount}
        />
      </div>

      <div class="demo-section">
        <h4>Sequential Scan Cost by Hardware</h4>
        <CostBarChart costs={scanCosts} />
      </div>

      <div class="demo-section">
        <h4>Join Cost and Algorithm Selection</h4>
        <CostBarChart costs={joinCosts} />
      </div>

      <div class="demo-section">
        <h4>Hardware Comparison Table</h4>
        <div class="demo-table-scroll">
          <table class="demo-table">
            <thead>
              <tr>
                <th>Hardware</th>
                <th>Cores</th>
                <th>Memory</th>
                <th>Storage</th>
                <th>GPU</th>
                <th>Join Algorithm</th>
                <th>Join Cost</th>
              </tr>
            </thead>
            <tbody>
              {results.map((r) => (
                <tr key={r.category}>
                  <td>{r.hw.name}</td>
                  <td>{r.hw.cpuCores}</td>
                  <td>{r.hw.memoryGb} GB</td>
                  <td>{r.hw.storageType}</td>
                  <td>{r.hw.hasGpu ? "Yes" : "No"}</td>
                  <td class="mono">
                    {r.joinAlg.replace(/_/g, " ")}
                  </td>
                  <td class="mono">{formatCost(r.joinCost.total)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div class="demo-section">
        <h4>Key Insight</h4>
        <div class="demo-insight">
          A Raspberry Pi with 4GB RAM must use Sort-Merge Join for large
          tables (hash table won't fit), while a data warehouse server
          with 2TB RAM always prefers Hash Join. NVMe storage can be
          100x faster than HDD for random I/O, making index scans
          viable where sequential scans dominate on spinning disks.
        </div>
      </div>
    </DemoCard>
  );
}

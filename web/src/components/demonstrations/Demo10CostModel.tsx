/**
 * Demo 10: Cost Model Calibration
 *
 * Interactive parameter tuning showing how cost model constants
 * affect plan choices. Users can adjust CPU cost per tuple, I/O
 * cost per page, random I/O multiplier, and see how plans change.
 */

import { useState } from "preact/hooks";
import type { CostBreakdown } from "src/components/demonstrations/types.ts";
import {
  formatCost,
  formatRows,
} from "src/components/demonstrations/optimizer.ts";
import {
  CostBarChart,
  DemoCard,
  Slider,
} from "src/components/demonstrations/DemoShared.tsx";

export function Demo10CostModel() {
  const [rowCount, setRowCount] = useState(10_000_000);
  const [cpuTupleCostNs, setCpuTupleCostNs] = useState(50);
  const [seqIoCostUs, setSeqIoCostUs] = useState(10);
  const [randomIoMultiplier, setRandomIoMultiplier] = useState(4);
  const [hashBuildCostNs, setHashBuildCostNs] = useState(100);
  const [hashProbeCostNs, setHashProbeCostNs] = useState(50);
  const [sortCompareCostNs, setSortCompareCostNs] = useState(200);

  const avgRowSize = 100;
  const pageSize = 8192;
  const rowsPerPage = Math.floor(pageSize / avgRowSize);
  const totalPages = Math.ceil(rowCount / rowsPerPage);
  const selectivity = 0.05;
  const selectedRows = Math.round(rowCount * selectivity);
  const selectedPages = Math.ceil(selectedRows / rowsPerPage);

  // Sequential scan cost
  const seqScanCpu = rowCount * cpuTupleCostNs * 1e-9;
  const seqScanIo = totalPages * seqIoCostUs * 1e-6;
  const seqScan: CostBreakdown = {
    cpu: seqScanCpu,
    io: seqScanIo,
    memory: 0,
    network: 0,
    total: seqScanCpu + seqScanIo,
  };

  // Index scan cost
  const indexScanCpu =
    selectedRows * cpuTupleCostNs * 1e-9 +
    Math.log2(rowCount + 1) * cpuTupleCostNs * 1e-9;
  const indexScanIo =
    selectedPages * seqIoCostUs * randomIoMultiplier * 1e-6;
  const indexScan: CostBreakdown = {
    cpu: indexScanCpu,
    io: indexScanIo,
    memory: 0,
    network: 0,
    total: indexScanCpu + indexScanIo,
  };

  // Hash join cost (build smaller side, probe larger)
  const buildRows = selectedRows;
  const probeRows = rowCount;
  const hashJoinCpuVal =
    buildRows * hashBuildCostNs * 1e-9 +
    probeRows * hashProbeCostNs * 1e-9;
  const hashJoinIo =
    (Math.ceil(buildRows / rowsPerPage) +
      Math.ceil(probeRows / rowsPerPage)) *
    seqIoCostUs *
    1e-6;
  const hashJoinMem = buildRows * avgRowSize * 2;
  const hashJoin: CostBreakdown = {
    cpu: hashJoinCpuVal,
    io: hashJoinIo,
    memory: hashJoinMem / 1e9,
    network: 0,
    total: hashJoinCpuVal + hashJoinIo + hashJoinMem / 1e9,
  };

  // Sort-merge join cost
  const nLogN =
    rowCount > 1 ? rowCount * Math.log2(rowCount) : rowCount;
  const sortCpu = nLogN * sortCompareCostNs * 1e-9;
  const sortIo = totalPages * seqIoCostUs * 1e-6 * 3; // read, write, re-read
  const sortMerge: CostBreakdown = {
    cpu: sortCpu,
    io: sortIo,
    memory: (rowCount * avgRowSize) / 1e9,
    network: 0,
    total: sortCpu + sortIo + (rowCount * avgRowSize) / 1e9,
  };

  // Determine which is cheapest for scan
  const scanWinner =
    seqScan.total < indexScan.total ? "Sequential Scan" : "Index Scan";
  const joinWinner =
    hashJoin.total < sortMerge.total ? "Hash Join" : "Sort-Merge Join";

  return (
    <DemoCard
      title="Cost Model Calibration"
      description="Tune the low-level cost model parameters to understand how they influence plan selection. These parameters are typically calibrated to match specific hardware through benchmarking."
    >
      <div class="demo-controls">
        <Slider
          label="Table rows"
          value={rowCount}
          min={10000}
          max={100_000_000}
          step={10000}
          format={formatRows}
          onChange={setRowCount}
        />
      </div>

      <div class="demo-section">
        <h4>Cost Model Parameters</h4>
        <div class="demo-controls">
          <Slider
            label="CPU cost per tuple"
            value={cpuTupleCostNs}
            min={1}
            max={500}
            format={(v) => `${v} ns`}
            onChange={setCpuTupleCostNs}
          />
          <Slider
            label="Sequential I/O cost per page"
            value={seqIoCostUs}
            min={1}
            max={1000}
            format={(v) => `${v} us`}
            onChange={setSeqIoCostUs}
          />
          <Slider
            label="Random I/O multiplier"
            value={randomIoMultiplier}
            min={1}
            max={100}
            format={(v) => `${v}x seq I/O`}
            onChange={setRandomIoMultiplier}
          />
          <Slider
            label="Hash build cost per tuple"
            value={hashBuildCostNs}
            min={10}
            max={1000}
            format={(v) => `${v} ns`}
            onChange={setHashBuildCostNs}
          />
          <Slider
            label="Hash probe cost per tuple"
            value={hashProbeCostNs}
            min={10}
            max={500}
            format={(v) => `${v} ns`}
            onChange={setHashProbeCostNs}
          />
          <Slider
            label="Sort comparison cost"
            value={sortCompareCostNs}
            min={10}
            max={2000}
            format={(v) => `${v} ns`}
            onChange={setSortCompareCostNs}
          />
        </div>
      </div>

      <div class="demo-metrics">
        <span class="demo-badge success">
          <span class="demo-badge-label">Scan Winner:</span>{" "}
          {scanWinner}
        </span>
        <span class="demo-badge success">
          <span class="demo-badge-label">Join Winner:</span>{" "}
          {joinWinner}
        </span>
        <span class="demo-badge default">
          <span class="demo-badge-label">Selectivity:</span> 5% (
          {formatRows(selectedRows)} rows)
        </span>
      </div>

      <div class="demo-section">
        <h4>Scan Method Costs</h4>
        <CostBarChart
          costs={[
            {
              label: `Sequential Scan${scanWinner === "Sequential Scan" ? " [WINNER]" : ""}`,
              cost: seqScan,
              highlight: scanWinner === "Sequential Scan",
            },
            {
              label: `Index Scan${scanWinner === "Index Scan" ? " [WINNER]" : ""}`,
              cost: indexScan,
              highlight: scanWinner === "Index Scan",
            },
          ]}
        />
      </div>

      <div class="demo-section">
        <h4>Join Method Costs</h4>
        <CostBarChart
          costs={[
            {
              label: `Hash Join${joinWinner === "Hash Join" ? " [WINNER]" : ""}`,
              cost: hashJoin,
              highlight: joinWinner === "Hash Join",
            },
            {
              label: `Sort-Merge Join${joinWinner === "Sort-Merge Join" ? " [WINNER]" : ""}`,
              cost: sortMerge,
              highlight: joinWinner === "Sort-Merge Join",
            },
          ]}
        />
      </div>

      <div class="demo-section">
        <h4>Computed Values</h4>
        <div class="demo-table-scroll">
          <table class="demo-table">
            <thead>
              <tr>
                <th>Metric</th>
                <th>Value</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Total pages (8KB each)</td>
                <td class="mono">{formatRows(totalPages)}</td>
              </tr>
              <tr>
                <td>Rows per page</td>
                <td class="mono">{rowsPerPage}</td>
              </tr>
              <tr>
                <td>Selected rows (5%)</td>
                <td class="mono">{formatRows(selectedRows)}</td>
              </tr>
              <tr>
                <td>Random I/O cost per page</td>
                <td class="mono">
                  {seqIoCostUs * randomIoMultiplier} us
                </td>
              </tr>
              <tr>
                <td>Hash table size</td>
                <td class="mono">
                  {((buildRows * avgRowSize * 2) / 1e6).toFixed(1)} MB
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="demo-section">
        <h4>Key Insight</h4>
        <div class="demo-insight">
          Cost model calibration determines crossover points. Increasing
          the random I/O multiplier (e.g., from 4x to 40x for HDD)
          makes sequential scans preferred over index scans at lower
          selectivities. Decreasing hash build cost (faster CPU) shifts
          the preference toward hash joins even for smaller data sets.
          Databases calibrate these values through micro-benchmarks on
          the actual hardware.
        </div>
      </div>
    </DemoCard>
  );
}

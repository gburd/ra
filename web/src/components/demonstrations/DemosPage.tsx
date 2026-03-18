/**
 * Demonstrations page: interactive explorations of how statistics
 * and hardware affect query optimizer decisions.
 *
 * Supports two modes:
 * - Simulation: client-side cost model mirroring ra-hardware/ra-stats
 * - WASM: live optimizer via ra-wasm (when built and loaded)
 */

import { useState } from "preact/hooks";
import { Demo01Staleness } from "src/components/demonstrations/Demo01Staleness.tsx";
import { Demo02Hardware } from "src/components/demonstrations/Demo02Hardware.tsx";
import { Demo03JoinSelection } from "src/components/demonstrations/Demo03JoinSelection.tsx";
import { Demo04Aggregation } from "src/components/demonstrations/Demo04Aggregation.tsx";
import { Demo05IndexSelection } from "src/components/demonstrations/Demo05IndexSelection.tsx";
import { Demo06Subquery } from "src/components/demonstrations/Demo06Subquery.tsx";
import { Demo07Parallel } from "src/components/demonstrations/Demo07Parallel.tsx";
import { Demo08GpuOffload } from "src/components/demonstrations/Demo08GpuOffload.tsx";
import { Demo09Distributed } from "src/components/demonstrations/Demo09Distributed.tsx";
import { Demo10CostModel } from "src/components/demonstrations/Demo10CostModel.tsx";
import { useOptimizer } from "src/hooks/useOptimizer.ts";
import {
  ALL_SCENARIOS,
  getScenarioSummary,
  getScenariosForDemo,
} from "src/components/demonstrations/test-scenarios.ts";

interface DemosPageProps {
  readonly path: string;
}

interface DemoEntry {
  readonly id: string;
  readonly title: string;
  readonly subtitle: string;
}

const DEMOS: readonly DemoEntry[] = [
  {
    id: "staleness",
    title: "1. Statistics Staleness",
    subtitle: "How stale stats change join plans",
  },
  {
    id: "hardware",
    title: "2. Hardware-Specific Plans",
    subtitle: "Same query, different hardware",
  },
  {
    id: "join",
    title: "3. Join Algorithm Selection",
    subtitle: "NL vs Hash vs Sort-Merge",
  },
  {
    id: "aggregation",
    title: "4. Aggregation Strategy",
    subtitle: "Hash vs Sort vs Streaming",
  },
  {
    id: "index",
    title: "5. Index Selection",
    subtitle: "Seq vs Index vs Bitmap scan",
  },
  {
    id: "subquery",
    title: "6. Subquery Unnesting",
    subtitle: "EXISTS to Semi Join",
  },
  {
    id: "parallel",
    title: "7. Parallel Execution",
    subtitle: "Serial vs parallel scaling",
  },
  {
    id: "gpu",
    title: "8. GPU Offloading",
    subtitle: "CPU vs GPU placement",
  },
  {
    id: "distributed",
    title: "9. Distributed Joins",
    subtitle: "Broadcast vs Shuffle",
  },
  {
    id: "costmodel",
    title: "10. Cost Model Tuning",
    subtitle: "Calibrate optimizer parameters",
  },
];

export function DemosPage(_props: DemosPageProps) {
  const [activeDemo, setActiveDemo] = useState<string>("staleness");
  const [showScenarios, setShowScenarios] = useState(false);
  const optimizer = useOptimizer();
  const summary = getScenarioSummary();
  const activeScenarios = getScenariosForDemo(activeDemo);

  return (
    <div class="demos-page">
      <div class="demos-sidebar">
        <div class="demos-sidebar-header">
          <h2>Demonstrations</h2>
          <p class="demos-sidebar-desc">
            Interactive explorations of query optimization
          </p>
        </div>

        {/* Optimizer mode toggle */}
        <div class="demos-mode-toggle">
          <span class="demos-mode-label">Engine:</span>
          <button
            class={`demos-mode-btn ${optimizer.mode === "simulation" ? "active" : ""}`}
            onClick={() => optimizer.setMode("simulation")}
          >
            Simulation
          </button>
          <button
            class={`demos-mode-btn ${optimizer.mode === "wasm" ? "active" : ""}`}
            onClick={() => optimizer.setMode("wasm")}
            disabled={!optimizer.wasmAvailable}
            title={
              optimizer.wasmAvailable
                ? `WASM v${optimizer.wasmStatus.version}`
                : "WASM optimizer not loaded"
            }
          >
            WASM
          </button>
        </div>

        {/* WASM status indicator */}
        <div class="demos-wasm-status">
          <span
            class={`demos-status-dot ${optimizer.wasmStatus.available ? "available" : optimizer.wasmStatus.loading ? "loading" : "unavailable"}`}
          />
          <span class="demos-status-text">
            {optimizer.wasmStatus.loading
              ? "Loading WASM..."
              : optimizer.wasmStatus.available
                ? `WASM v${optimizer.wasmStatus.version}`
                : "WASM: stub mode"}
          </span>
        </div>

        <nav class="demos-nav">
          {DEMOS.map((demo) => (
            <button
              key={demo.id}
              class={`demos-nav-item ${activeDemo === demo.id ? "active" : ""}`}
              onClick={() => setActiveDemo(demo.id)}
            >
              <span class="demos-nav-title">{demo.title}</span>
              <span class="demos-nav-subtitle">{demo.subtitle}</span>
            </button>
          ))}
        </nav>

        {/* Test scenarios toggle */}
        <div class="demos-scenarios-toggle">
          <button
            class="demos-scenarios-btn"
            onClick={() => setShowScenarios(!showScenarios)}
          >
            {showScenarios ? "Hide" : "Show"} Test Scenarios
            ({summary.total} total)
          </button>
        </div>
      </div>

      <div class="demos-content">
        {/* Active demo */}
        {activeDemo === "staleness" && <Demo01Staleness />}
        {activeDemo === "hardware" && <Demo02Hardware />}
        {activeDemo === "join" && <Demo03JoinSelection />}
        {activeDemo === "aggregation" && <Demo04Aggregation />}
        {activeDemo === "index" && <Demo05IndexSelection />}
        {activeDemo === "subquery" && <Demo06Subquery />}
        {activeDemo === "parallel" && <Demo07Parallel />}
        {activeDemo === "gpu" && <Demo08GpuOffload />}
        {activeDemo === "distributed" && <Demo09Distributed />}
        {activeDemo === "costmodel" && <Demo10CostModel />}

        {/* Test scenarios panel */}
        {showScenarios && activeScenarios.length > 0 && (
          <div class="demos-scenarios-panel">
            <h3>Test Scenarios for this Demo</h3>
            <table class="demos-scenarios-table">
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Name</th>
                  <th>Expected</th>
                </tr>
              </thead>
              <tbody>
                {activeScenarios.map((s) => (
                  <tr key={s.id}>
                    <td class="demos-scenario-id">{s.id}</td>
                    <td>
                      <strong>{s.name}</strong>
                      <br />
                      <span class="demos-scenario-desc">
                        {s.description}
                      </span>
                    </td>
                    <td class="demos-scenario-expected">
                      {s.expected.primaryResult}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}

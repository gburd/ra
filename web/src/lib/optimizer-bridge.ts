/**
 * Bridge between the demo components and the WASM query optimizer.
 *
 * Provides a unified interface that either uses the live WASM optimizer
 * (when available) or falls back to the client-side simulation engine.
 * Demos can toggle between modes at runtime.
 */

import type {
  CostBreakdown,
  HardwareCategory,
  HardwareConfig,
  SimPlanNode,
} from "src/components/demonstrations/types.ts";

/** Optimization mode: live WASM or client-side simulation. */
export type OptimizerMode = "simulation" | "wasm";

/** Result from the optimizer bridge. */
export interface BridgeOptimizationResult {
  readonly originalPlan: SimPlanNode;
  readonly optimizedPlan: SimPlanNode;
  readonly originalCost: CostBreakdown;
  readonly optimizedCost: CostBreakdown;
  readonly improvement: number;
  readonly mode: OptimizerMode;
  readonly timeMs: number;
  readonly egraphNodes: number;
  readonly appliedRules: readonly string[];
  readonly hardwareProfileName: string;
}

/** Table statistics for the optimizer. */
export interface TableStats {
  readonly table: string;
  readonly rowCount: number;
  readonly avgRowSize: number;
  readonly distinctCount: number;
  readonly nullFraction: number;
}

/** Status of the WASM optimizer. */
export interface WasmStatus {
  readonly available: boolean;
  readonly loading: boolean;
  readonly error: string | null;
  readonly version: string | null;
}

/** Messages sent to the optimizer worker. */
interface WorkerRequest {
  readonly id: number;
  readonly type:
    | "init"
    | "optimize_sql"
    | "optimize_plan"
    | "set_hardware"
    | "set_stats"
    | "clear_stats"
    | "get_version"
    | "get_status";
  readonly payload?: string;
}

/** Messages received from the optimizer worker. */
interface WorkerResponse {
  readonly id: number;
  readonly success: boolean;
  readonly result?: string;
  readonly error?: string;
}

type PendingRequest = {
  resolve: (value: string) => void;
  reject: (reason: Error) => void;
};

/**
 * Optimizer bridge that manages the WASM optimizer worker and provides
 * a clean API for the demo components.
 */
export class OptimizerBridge {
  private worker: Worker | null = null;
  private nextId = 1;
  private pending = new Map<number, PendingRequest>();
  private _status: WasmStatus = {
    available: false,
    loading: false,
    error: null,
    version: null,
  };
  private statusListeners: Array<(status: WasmStatus) => void> = [];

  get status(): WasmStatus {
    return this._status;
  }

  /** Register a callback for status changes. */
  onStatusChange(listener: (status: WasmStatus) => void): () => void {
    this.statusListeners.push(listener);
    return () => {
      const idx = this.statusListeners.indexOf(listener);
      if (idx >= 0) {
        this.statusListeners.splice(idx, 1);
      }
    };
  }

  private updateStatus(partial: Partial<WasmStatus>): void {
    this._status = { ...this._status, ...partial };
    for (const listener of this.statusListeners) {
      listener(this._status);
    }
  }

  /** Initialize the WASM optimizer worker. */
  async init(): Promise<boolean> {
    if (typeof WebAssembly === "undefined") {
      this.updateStatus({
        available: false,
        error: "WebAssembly not supported",
      });
      return false;
    }

    this.updateStatus({ loading: true });

    try {
      this.worker = new Worker(
        new URL(
          "src/workers/optimizer-worker.ts",
          import.meta.url,
        ),
        { type: "module" },
      );

      this.worker.onmessage = (event: MessageEvent) => {
        const response = event.data as WorkerResponse;
        const pending = this.pending.get(response.id);
        if (pending === undefined) return;

        this.pending.delete(response.id);
        if (response.success) {
          pending.resolve(response.result ?? "");
        } else {
          pending.reject(
            new Error(response.error ?? "Unknown worker error"),
          );
        }
      };

      this.worker.onerror = (event: ErrorEvent) => {
        this.updateStatus({
          available: false,
          loading: false,
          error: event.message,
        });
        for (const [id, pending] of this.pending) {
          pending.reject(new Error(`Worker error: ${event.message}`));
          this.pending.delete(id);
        }
      };

      const result = await this.send({ type: "init" });
      const version = JSON.parse(result).version as string;
      this.updateStatus({
        available: true,
        loading: false,
        version,
        error: null,
      });
      return true;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.updateStatus({
        available: false,
        loading: false,
        error: msg,
      });
      return false;
    }
  }

  /** Set the hardware profile for the WASM optimizer. */
  async setHardwareProfile(
    _category: HardwareCategory,
    config: HardwareConfig,
  ): Promise<void> {
    if (this.worker === null) return;
    const profile = hardwareConfigToProfile(config);
    await this.send({
      type: "set_hardware",
      payload: JSON.stringify(profile),
    });
  }

  /** Add table statistics. */
  async addTableStats(stats: TableStats): Promise<void> {
    if (this.worker === null) return;
    await this.send({
      type: "set_stats",
      payload: JSON.stringify(stats),
    });
  }

  /** Clear all table statistics. */
  async clearTableStats(): Promise<void> {
    if (this.worker === null) return;
    await this.send({ type: "clear_stats" });
  }

  /** Optimize a SQL query using the WASM optimizer. */
  async optimizeSQL(
    sql: string,
  ): Promise<BridgeOptimizationResult> {
    if (this.worker === null) {
      throw new Error("WASM optimizer not initialized");
    }
    const resultJson = await this.send({
      type: "optimize_sql",
      payload: sql,
    });
    return parseOptimizationResult(resultJson, "wasm");
  }

  /** Optimize a plan (as JSON) using the WASM optimizer. */
  async optimizePlan(
    planJson: string,
  ): Promise<BridgeOptimizationResult> {
    if (this.worker === null) {
      throw new Error("WASM optimizer not initialized");
    }
    const resultJson = await this.send({
      type: "optimize_plan",
      payload: planJson,
    });
    return parseOptimizationResult(resultJson, "wasm");
  }

  /** Terminate the worker. */
  terminate(): void {
    if (this.worker === null) return;
    this.worker.terminate();
    this.worker = null;
    this.updateStatus({
      available: false,
      loading: false,
      version: null,
    });
    for (const [, pending] of this.pending) {
      pending.reject(new Error("Worker terminated"));
    }
    this.pending.clear();
  }

  private send(
    msg: Omit<WorkerRequest, "id">,
  ): Promise<string> {
    if (this.worker === null) {
      return Promise.reject(new Error("Worker not initialized"));
    }

    const id = this.nextId++;
    const message: WorkerRequest = { ...msg, id };

    return new Promise<string>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.worker?.postMessage(message);
    });
  }
}

/** Convert TypeScript HardwareConfig to the Rust HardwareProfile shape. */
function hardwareConfigToProfile(config: HardwareConfig): Record<string, unknown> {
  return {
    name: config.name,
    cpu_available: true,
    cpu_cores: config.cpuCores,
    cpu_memory_bandwidth_gbps: config.cpuMemoryBandwidthGbps,
    l2_cache_bytes: 1_048_576,
    l3_cache_bytes: config.l3CacheMb * 1024 * 1024,
    l3_latency_ns: 35.0,
    dram_latency_ns: 90.0,
    simd_width_bits: config.simdWidthBits,
    numa_nodes: config.numaNodes,
    memory_level_parallelism: 16,
    gpu_available: config.hasGpu,
    gpu_memory_bytes: config.gpuMemoryGb * 1024 * 1024 * 1024,
    gpu_memory_bandwidth_gbps: config.gpuMemoryBandwidthGbps,
    gpu_sm_count: config.gpuSmCount,
    unified_memory_supported: config.hasGpu,
    page_migration_engine_available: config.hasGpu,
    um_page_size_bytes: 65_536,
    um_fault_latency_us: 20.0,
    um_migration_bandwidth_gbps: 12.0,
    chunked_transfer_enabled: config.hasGpu,
    fpga_available: false,
    fpga_clock_mhz: 0,
    fpga_bram_bytes: 0,
    fpga_max_pipeline_depth: 0,
    fpga_reconfig_ms: 0,
    fpga_near_storage: false,
    fpga_available_luts: 0,
    fpga_regex_engines: 0,
    pcie_bandwidth_gbps: config.pcieBandwidthGbps,
    storage_bandwidth_gbps: config.storageBandwidthGbps,
  };
}

/** Parse the JSON result from the WASM optimizer into the bridge format. */
function parseOptimizationResult(
  json: string,
  mode: OptimizerMode,
): BridgeOptimizationResult {
  const raw = JSON.parse(json) as {
    original_plan: unknown;
    optimized_plan: unknown;
    original_cost: number;
    optimized_cost: number;
    original_cost_breakdown: {
      cpu: number;
      io: number;
      memory: number;
      network: number;
      total: number;
    };
    optimized_cost_breakdown: {
      cpu: number;
      io: number;
      memory: number;
      network: number;
      total: number;
    };
    improvement: number;
    iterations: number;
    egraph_nodes: number;
    time_ms: number;
    applied_rules: string[];
    hardware_profile_name: string;
  };

  return {
    originalPlan: planJsonToSimNode(raw.original_plan),
    optimizedPlan: planJsonToSimNode(raw.optimized_plan),
    originalCost: raw.original_cost_breakdown,
    optimizedCost: raw.optimized_cost_breakdown,
    improvement: raw.improvement,
    mode,
    timeMs: raw.time_ms,
    egraphNodes: raw.egraph_nodes,
    appliedRules: raw.applied_rules,
    hardwareProfileName: raw.hardware_profile_name,
  };
}

/** Convert a RelExpr JSON node into a SimPlanNode for visualization. */
function planJsonToSimNode(node: unknown): SimPlanNode {
  if (node === null || node === undefined || typeof node !== "object") {
    return {
      operator: "Unknown",
      cost: { cpu: 0, io: 0, memory: 0, network: 0, total: 0 },
      estimatedRows: 0,
      properties: {},
      children: [],
    };
  }

  const obj = node as Record<string, unknown>;
  const keys = Object.keys(obj);
  if (keys.length === 0) {
    return {
      operator: "Empty",
      cost: { cpu: 0, io: 0, memory: 0, network: 0, total: 0 },
      estimatedRows: 0,
      properties: {},
      children: [],
    };
  }

  const operator = keys[0] ?? "Unknown";
  const inner = obj[operator] as Record<string, unknown> | undefined;
  const children: SimPlanNode[] = [];
  const properties: Record<string, string> = {};

  if (inner !== undefined && typeof inner === "object") {
    for (const [key, value] of Object.entries(inner)) {
      if (
        value !== null &&
        typeof value === "object" &&
        !Array.isArray(value)
      ) {
        children.push(planJsonToSimNode(value));
      } else if (key === "input" || key === "left" || key === "right") {
        children.push(planJsonToSimNode(value));
      } else {
        properties[key] = String(value);
      }
    }
  }

  return {
    operator: formatOperator(operator),
    cost: { cpu: 0, io: 0, memory: 0, network: 0, total: 0 },
    estimatedRows: 0,
    properties,
    children,
  };
}

function formatOperator(raw: string): string {
  return raw
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

/** Singleton bridge instance. */
let bridgeInstance: OptimizerBridge | null = null;

/** Get or create the global optimizer bridge. */
export function getOptimizerBridge(): OptimizerBridge {
  if (bridgeInstance === null) {
    bridgeInstance = new OptimizerBridge();
  }
  return bridgeInstance;
}

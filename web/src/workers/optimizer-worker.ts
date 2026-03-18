/**
 * Web worker for the WASM query optimizer.
 *
 * Runs the ra-wasm optimizer in a background thread to keep the
 * main thread responsive. Handles initialization, configuration
 * changes, and optimization requests.
 *
 * When the WASM module is not yet built, this worker returns
 * simulated results so demos remain functional.
 */

// Module marker: ensures TypeScript treats this as a module.
export type OptimizerWorkerSelf = typeof self;

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

interface WorkerResponse {
  readonly id: number;
  readonly success: boolean;
  readonly result?: string;
  readonly error?: string;
}

/**
 * Dynamic WASM module interface. When the WASM build is available,
 * the init function loads it. Until then, we use stubs.
 */
interface WasmModule {
  WasmOptimizer: {
    new (): WasmOptimizerInstance;
  };
}

interface WasmOptimizerInstance {
  setConfig(configJson: string): void;
  setHardwareProfile(profileJson: string): void;
  addTableStats(statsJson: string): void;
  clearTableStats(): void;
  optimizeSQL(sql: string): string;
  optimizePlanJSON(planJson: string): string;
  getVersion(): string;
}

let optimizer: WasmOptimizerInstance | null = null;
let wasmAvailable = false;
const version = "0.1.0-stub";

self.onmessage = (event: MessageEvent) => {
  const msg = event.data as WorkerRequest;
  const response = handleMessage(msg);
  self.postMessage(response);
};

function handleMessage(msg: WorkerRequest): WorkerResponse {
  try {
    switch (msg.type) {
      case "init":
        return handleInit(msg);
      case "optimize_sql":
        return handleOptimizeSQL(msg);
      case "optimize_plan":
        return handleOptimizePlan(msg);
      case "set_hardware":
        return handleSetHardware(msg);
      case "set_stats":
        return handleSetStats(msg);
      case "clear_stats":
        return handleClearStats(msg);
      case "get_version":
        return handleGetVersion(msg);
      case "get_status":
        return handleGetStatus(msg);
    }
  } catch (err) {
    return {
      id: msg.id,
      success: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

function handleInit(msg: WorkerRequest): WorkerResponse {
  // Attempt to load WASM module. If not available, use stub mode.
  try {
    // WASM module path will be configured at build time.
    // For now, operate in stub mode with simulated results.
    wasmAvailable = false;
    optimizer = createStubOptimizer();

    return {
      id: msg.id,
      success: true,
      result: JSON.stringify({
        version: wasmAvailable ? "live" : version,
        mode: wasmAvailable ? "wasm" : "stub",
      }),
    };
  } catch (err) {
    return {
      id: msg.id,
      success: false,
      error: `Init failed: ${err instanceof Error ? err.message : String(err)}`,
    };
  }
}

function handleOptimizeSQL(msg: WorkerRequest): WorkerResponse {
  if (optimizer === null) {
    return {
      id: msg.id,
      success: false,
      error: "Optimizer not initialized",
    };
  }

  const sql = msg.payload ?? "";
  const result = optimizer.optimizeSQL(sql);
  return { id: msg.id, success: true, result };
}

function handleOptimizePlan(msg: WorkerRequest): WorkerResponse {
  if (optimizer === null) {
    return {
      id: msg.id,
      success: false,
      error: "Optimizer not initialized",
    };
  }

  const planJson = msg.payload ?? "{}";
  const result = optimizer.optimizePlanJSON(planJson);
  return { id: msg.id, success: true, result };
}

function handleSetHardware(msg: WorkerRequest): WorkerResponse {
  if (optimizer === null) {
    return { id: msg.id, success: false, error: "Not initialized" };
  }

  optimizer.setHardwareProfile(msg.payload ?? "{}");
  return { id: msg.id, success: true };
}

function handleSetStats(msg: WorkerRequest): WorkerResponse {
  if (optimizer === null) {
    return { id: msg.id, success: false, error: "Not initialized" };
  }

  optimizer.addTableStats(msg.payload ?? "{}");
  return { id: msg.id, success: true };
}

function handleClearStats(msg: WorkerRequest): WorkerResponse {
  if (optimizer === null) {
    return { id: msg.id, success: false, error: "Not initialized" };
  }

  optimizer.clearTableStats();
  return { id: msg.id, success: true };
}

function handleGetVersion(msg: WorkerRequest): WorkerResponse {
  const v = optimizer !== null
    ? optimizer.getVersion()
    : version;
  return { id: msg.id, success: true, result: v };
}

function handleGetStatus(msg: WorkerRequest): WorkerResponse {
  return {
    id: msg.id,
    success: true,
    result: JSON.stringify({
      wasm_available: wasmAvailable,
      optimizer_ready: optimizer !== null,
      version: wasmAvailable ? "live" : version,
    }),
  };
}

/**
 * Create a stub optimizer that returns simulated results.
 * Used when the WASM module is not yet built.
 */
function createStubOptimizer(): WasmOptimizerInstance {
  let hardwareName = "auto-detect (stub)";

  return {
    setConfig(_configJson: string): void {
      // Stub: accept but ignore
    },

    setHardwareProfile(profileJson: string): void {
      try {
        const profile = JSON.parse(profileJson) as { name?: string };
        hardwareName = profile.name ?? "custom";
      } catch {
        // Ignore parse errors in stub
      }
    },

    addTableStats(_statsJson: string): void {
      // Stub: accept but ignore
    },

    clearTableStats(): void {
      // Stub: no-op
    },

    optimizeSQL(sql: string): string {
      return generateStubResult(sql, hardwareName);
    },

    optimizePlanJSON(planJson: string): string {
      return generateStubPlanResult(planJson, hardwareName);
    },

    getVersion(): string {
      return version;
    },
  };
}

/** Generate a simulated optimization result for a SQL query. */
function generateStubResult(
  sql: string,
  hwName: string,
): string {
  const lowerSql = sql.toLowerCase();
  const hasJoin = lowerSql.includes("join");
  const hasWhere = lowerSql.includes("where");
  const hasGroup = lowerSql.includes("group by");
  const hasOrder = lowerSql.includes("order by");

  let baseCost = 100;
  if (hasJoin) baseCost += 500;
  if (hasWhere) baseCost += 10;
  if (hasGroup) baseCost += 200;
  if (hasOrder) baseCost += 150;

  const improvement = hasJoin ? 0.35 : hasGroup ? 0.25 : 0.1;
  const optimizedCost = baseCost * (1 - improvement);

  return JSON.stringify({
    original_plan: { Scan: { table: "stub_table", alias: null } },
    optimized_plan: { Scan: { table: "stub_table", alias: null } },
    original_cost: baseCost,
    optimized_cost: optimizedCost,
    original_cost_breakdown: {
      cpu: baseCost * 0.4,
      io: baseCost * 0.4,
      memory: baseCost * 0.2,
      network: 0,
      total: baseCost,
    },
    optimized_cost_breakdown: {
      cpu: optimizedCost * 0.4,
      io: optimizedCost * 0.4,
      memory: optimizedCost * 0.2,
      network: 0,
      total: optimizedCost,
    },
    improvement,
    iterations: 10,
    egraph_nodes: 42,
    time_ms: 5,
    applied_rules: hasJoin
      ? ["join-commutativity", "predicate-pushdown"]
      : ["predicate-pushdown"],
    hardware_profile_name: hwName,
  });
}

/** Generate a simulated optimization result for a plan JSON. */
function generateStubPlanResult(
  _planJson: string,
  hwName: string,
): string {
  return JSON.stringify({
    original_plan: { Scan: { table: "stub", alias: null } },
    optimized_plan: { Scan: { table: "stub", alias: null } },
    original_cost: 100,
    optimized_cost: 80,
    original_cost_breakdown: {
      cpu: 40, io: 40, memory: 20, network: 0, total: 100,
    },
    optimized_cost_breakdown: {
      cpu: 32, io: 32, memory: 16, network: 0, total: 80,
    },
    improvement: 0.2,
    iterations: 10,
    egraph_nodes: 30,
    time_ms: 3,
    applied_rules: ["predicate-pushdown"],
    hardware_profile_name: hwName,
  });
}

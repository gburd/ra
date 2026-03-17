/**
 * Web worker for running WASM-compiled databases.
 *
 * This worker receives messages from the main thread WasmClient
 * and executes SQL queries against SQLite or DuckDB compiled to
 * WASM. Running in a worker keeps the main thread responsive.
 *
 * The actual WASM module loading is stubbed out here. When the
 * WASM builds (Task #2) are integrated, this worker will load
 * the `.wasm` binaries and provide real execution.
 */

interface WorkerMessage {
  readonly id: number;
  readonly type: "execute" | "init" | "reset";
  readonly database: string;
  readonly sql?: string;
}

interface WorkerResponse {
  readonly id: number;
  readonly success: boolean;
  readonly result?: {
    readonly columns: readonly { name: string; type: string }[];
    readonly rows: readonly Record<string, unknown>[];
    readonly elapsed_ms: number;
    readonly row_count: number;
  };
  readonly error?: string;
}

const initializedDbs = new Set<string>();

self.onmessage = (event: MessageEvent) => {
  const msg = event.data as WorkerMessage;
  const response = handleMessage(msg);
  self.postMessage(response);
};

function handleMessage(msg: WorkerMessage): WorkerResponse {
  switch (msg.type) {
    case "init":
      return handleInit(msg);
    case "execute":
      return handleExecute(msg);
    case "reset":
      return handleReset(msg);
  }
}

function handleInit(msg: WorkerMessage): WorkerResponse {
  // Stub: in production, this would load the WASM module
  initializedDbs.add(msg.database);
  return {
    id: msg.id,
    success: true,
    result: {
      columns: [],
      rows: [],
      elapsed_ms: 0,
      row_count: 0,
    },
  };
}

function handleExecute(msg: WorkerMessage): WorkerResponse {
  if (!initializedDbs.has(msg.database)) {
    return {
      id: msg.id,
      success: false,
      error: `Database ${msg.database} not initialized`,
    };
  }

  // Stub: in production, this executes SQL against the WASM db
  const sql = msg.sql?.toLowerCase() ?? "";
  if (sql.includes("select")) {
    return {
      id: msg.id,
      success: true,
      result: {
        columns: [{ name: "result", type: "TEXT" }],
        rows: [{ result: `[WASM ${msg.database}] stub result` }],
        elapsed_ms: 0.1,
        row_count: 1,
      },
    };
  }

  return {
    id: msg.id,
    success: true,
    result: {
      columns: [],
      rows: [],
      elapsed_ms: 0.1,
      row_count: 0,
    },
  };
}

function handleReset(msg: WorkerMessage): WorkerResponse {
  initializedDbs.delete(msg.database);
  return {
    id: msg.id,
    success: true,
    result: {
      columns: [],
      rows: [],
      elapsed_ms: 0,
      row_count: 0,
    },
  };
}

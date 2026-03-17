// DuckDB WASM bridge for ra-wasm.
//
// This module wraps @duckdb/duckdb-wasm, managing database handles
// and translating between the Rust adapter's JSON-based protocol
// and the DuckDB async connection API.
//
// DuckDB WASM has an async API, but wasm-bindgen extern functions
// are synchronous. The bridge pre-initializes the database during
// `initDuckDb()` and then uses synchronous wrappers where possible.
// For truly async operations (rare in the query path after init),
// the JS caller must await initialization before Rust calls begin.

let nextHandle = 1;
const connections = new Map();
let duckdb = null;
let db = null;

async function ensureDuckDb() {
  if (duckdb && db) return { duckdb, db };

  const duckdbMod = await import("@duckdb/duckdb-wasm");
  const DUCKDB_BUNDLES = duckdbMod.getJsDelivrBundles();

  const bundle = await duckdbMod.selectBundle(DUCKDB_BUNDLES);
  const worker = new Worker(bundle.mainWorker);
  const logger = new duckdbMod.ConsoleLogger();

  duckdb = new duckdbMod.AsyncDuckDB(logger, worker);
  await duckdb.instantiate(bundle.mainModule, bundle.pthreadWorker);
  db = await duckdb.open({});

  return { duckdb, db };
}

function serializeResult(columns, rows, rowsAffected) {
  return JSON.stringify({
    columns: columns.map((name) => ({ name, type_name: null })),
    rows,
    rows_affected: rowsAffected,
  });
}

function arrowToRows(result) {
  const schema = result.schema;
  const columns = schema.fields.map((f) => f.name);
  const rows = [];
  const batches = result.batches || [result];

  for (const batch of batches) {
    const numRows = batch.numRows;
    for (let r = 0; r < numRows; r++) {
      const row = [];
      for (let c = 0; c < columns.length; c++) {
        const col = batch.getChildAt(c);
        const val = col.get(r);

        if (val === null || val === undefined) {
          row.push("Null");
        } else if (typeof val === "bigint") {
          row.push({ Integer: Number(val) });
        } else if (typeof val === "number") {
          row.push(
            Number.isInteger(val)
              ? { Integer: val }
              : { Float: val }
          );
        } else if (typeof val === "boolean") {
          row.push({ Boolean: val });
        } else if (typeof val === "string") {
          row.push({ Text: val });
        } else if (val instanceof Uint8Array) {
          row.push({ Blob: Array.from(val) });
        } else {
          row.push({ Text: String(val) });
        }
      }
      rows.push(row);
    }
  }

  return { columns, rows };
}

function valuesToParams(paramsJson) {
  const params = JSON.parse(paramsJson);
  return params.map((v) => {
    if (v === "Null" || v === null) return null;
    if (typeof v === "object") {
      if ("Integer" in v) return v.Integer;
      if ("Float" in v) return v.Float;
      if ("Text" in v) return v.Text;
      if ("Boolean" in v) return v.Boolean;
      if ("Blob" in v) return new Uint8Array(v.Blob);
    }
    return v;
  });
}

export function duckdbOpen(configJson) {
  if (!db) {
    throw new Error(
      "DuckDB WASM not initialized. Call initDuckDb() first."
    );
  }

  // DuckDB WASM connections are created synchronously after the
  // database instance is initialized.
  const conn = db.connect();
  const handle = nextHandle++;
  connections.set(handle, conn);
  return handle;
}

export function duckdbExec(handle, sql) {
  const conn = connections.get(handle);
  if (!conn) throw new Error(`Unknown DuckDB handle: ${handle}`);

  conn.query(sql);
  return serializeResult([], [], 0);
}

export function duckdbQuery(handle, sql) {
  const conn = connections.get(handle);
  if (!conn) throw new Error(`Unknown DuckDB handle: ${handle}`);

  const result = conn.query(sql);
  const { columns, rows } = arrowToRows(result);
  return serializeResult(columns, rows, 0);
}

export function duckdbExecParams(handle, sql, paramsJson) {
  const conn = connections.get(handle);
  if (!conn) throw new Error(`Unknown DuckDB handle: ${handle}`);

  const params = valuesToParams(paramsJson);
  const stmt = conn.prepare(sql);
  stmt.query(...params);
  stmt.close();

  return serializeResult([], [], 0);
}

export function duckdbQueryParams(handle, sql, paramsJson) {
  const conn = connections.get(handle);
  if (!conn) throw new Error(`Unknown DuckDB handle: ${handle}`);

  const params = valuesToParams(paramsJson);
  const stmt = conn.prepare(sql);
  const result = stmt.query(...params);
  stmt.close();

  const { columns, rows } = arrowToRows(result);
  return serializeResult(columns, rows, 0);
}

export function duckdbClose(handle) {
  const conn = connections.get(handle);
  if (!conn) return;
  conn.close();
  connections.delete(handle);
}

// Async initialization entry point. Must be called once before
// any duckdbOpen() call.
export async function initDuckDb() {
  await ensureDuckDb();
}

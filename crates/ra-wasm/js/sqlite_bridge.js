// SQLite WASM bridge for ra-wasm.
//
// This module wraps @sqlite.org/sqlite-wasm, managing database
// handles and translating between the Rust adapter's JSON-based
// protocol and the sqlite3 C-style API exposed by the WASM build.
//
// Each opened database gets a numeric handle that the Rust side
// passes back on subsequent calls. The bridge maintains a handle
// map so the Rust code never touches JS objects directly.

let nextHandle = 1;
const databases = new Map();
let sqlite3 = null;

async function ensureSqlite3() {
  if (sqlite3) return sqlite3;
  // Dynamic import so the bridge file can be loaded before the
  // WASM binary is available on the page.
  const mod = await import("@sqlite.org/sqlite-wasm");
  sqlite3 = await mod.default("sqlite3");
  return sqlite3;
}

function serializeResult(columns, rows, rowsAffected) {
  return JSON.stringify({
    columns: columns.map((name) => ({ name, type_name: null })),
    rows,
    rows_affected: rowsAffected,
  });
}

function valuesToBindings(paramsJson) {
  const params = JSON.parse(paramsJson);
  return params.map((v) => {
    if (v === "Null" || v === null) return null;
    if (typeof v === "object") {
      if ("Integer" in v) return v.Integer;
      if ("Float" in v) return v.Float;
      if ("Text" in v) return v.Text;
      if ("Boolean" in v) return v.Boolean ? 1 : 0;
      if ("Blob" in v) return new Uint8Array(v.Blob);
    }
    return v;
  });
}

export function sqliteOpen(configJson) {
  // ensureSqlite3 is async but wasm-bindgen extern fns are sync.
  // The caller must ensure sqlite3 has been initialized before
  // calling open. See `initSqlite()` below.
  if (!sqlite3) {
    throw new Error(
      "SQLite WASM not initialized. Call initSqlite() first."
    );
  }

  const config = JSON.parse(configJson);
  let filename = ":memory:";
  if (config.database_name && config.storage !== "Memory") {
    filename = config.database_name;
  }

  const flags = config.read_only ? "r" : "cw";
  let db;

  if (config.storage === "Opfs" && sqlite3.oo1.OpfsDb) {
    db = new sqlite3.oo1.OpfsDb(filename, flags);
  } else {
    db = new sqlite3.oo1.DB(filename, flags);
  }

  const handle = nextHandle++;
  databases.set(handle, db);
  return handle;
}

export function sqliteExec(handle, sql) {
  const db = databases.get(handle);
  if (!db) throw new Error(`Unknown SQLite handle: ${handle}`);

  db.exec(sql);
  const changes = db.changes();
  return serializeResult([], [], changes);
}

export function sqliteQuery(handle, sql) {
  const db = databases.get(handle);
  if (!db) throw new Error(`Unknown SQLite handle: ${handle}`);

  const stmt = db.prepare(sql);
  const columns = stmt.getColumnNames();
  const rows = [];

  try {
    while (stmt.step()) {
      const row = [];
      for (let i = 0; i < columns.length; i++) {
        const val = stmt.get(i);
        if (val === null || val === undefined) {
          row.push("Null");
        } else if (typeof val === "number") {
          row.push(
            Number.isInteger(val)
              ? { Integer: val }
              : { Float: val }
          );
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
  } finally {
    stmt.finalize();
  }

  return serializeResult(columns, rows, 0);
}

export function sqliteExecParams(handle, sql, paramsJson) {
  const db = databases.get(handle);
  if (!db) throw new Error(`Unknown SQLite handle: ${handle}`);

  const bindings = valuesToBindings(paramsJson);
  db.exec({ sql, bind: bindings });
  const changes = db.changes();
  return serializeResult([], [], changes);
}

export function sqliteQueryParams(handle, sql, paramsJson) {
  const db = databases.get(handle);
  if (!db) throw new Error(`Unknown SQLite handle: ${handle}`);

  const bindings = valuesToBindings(paramsJson);
  const stmt = db.prepare(sql);

  try {
    stmt.bind(bindings);
  } catch (e) {
    stmt.finalize();
    throw e;
  }

  const columns = stmt.getColumnNames();
  const rows = [];

  try {
    while (stmt.step()) {
      const row = [];
      for (let i = 0; i < columns.length; i++) {
        const val = stmt.get(i);
        if (val === null || val === undefined) {
          row.push("Null");
        } else if (typeof val === "number") {
          row.push(
            Number.isInteger(val)
              ? { Integer: val }
              : { Float: val }
          );
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
  } finally {
    stmt.finalize();
  }

  return serializeResult(columns, rows, 0);
}

export function sqliteClose(handle) {
  const db = databases.get(handle);
  if (!db) return;
  db.close();
  databases.delete(handle);
}

// Async initialization entry point. Must be called once before
// any sqliteOpen() call. This is invoked from JS (not from Rust)
// during application startup.
export async function initSqlite() {
  await ensureSqlite3();
}

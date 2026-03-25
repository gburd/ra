/** sql.js wrapper for in-browser SQLite execution. */

import type { Database, SqlJsStatic } from "sql.js";

let sqlPromise: Promise<SqlJsStatic> | null = null;
let db: Database | null = null;

function getSqlJs(): Promise<SqlJsStatic> {
  if (!sqlPromise) {
    sqlPromise = import("sql.js").then((SQL) =>
      SQL.default({
        locateFile: (file: string) =>
          `https://sql.js.org/dist/${file}`,
      }),
    );
  }
  return sqlPromise;
}

export async function initDb(): Promise<Database> {
  if (db) return db;
  const SQL = await getSqlJs();
  db = new SQL.Database();
  return db;
}

export interface QueryResult {
  columns: string[];
  rows: string[][];
  rowCount: number;
  timeMs: number;
}

export async function executeSQL(sql: string): Promise<QueryResult> {
  const database = await initDb();
  const start = performance.now();

  const statements = sql
    .split(";")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);

  let lastResult: QueryResult = {
    columns: [],
    rows: [],
    rowCount: 0,
    timeMs: 0,
  };

  for (const stmt of statements) {
    const results = database.exec(stmt);
    if (results.length > 0) {
      const result = results[results.length - 1];
      if (result) {
        lastResult = {
          columns: result.columns,
          rows: result.values.map((row) =>
            row.map((v) => (v === null ? "NULL" : String(v))),
          ),
          rowCount: result.values.length,
          timeMs: 0,
        };
      }
    }
  }

  lastResult.timeMs = performance.now() - start;
  return lastResult;
}

export function resetDb(): void {
  if (db) {
    db.close();
    db = null;
  }
}

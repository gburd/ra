/** Supported database backends. */
export type DatabaseId =
  | "sqlite"
  | "duckdb"
  | "postgresql"
  | "mysql"
  | "sqlserver";

/** A database option for the selector UI. */
export interface DatabaseOption {
  readonly id: DatabaseId;
  readonly name: string;
  readonly color: string;
  readonly available: boolean;
}

/** All supported databases. */
export const DATABASES: readonly DatabaseOption[] = [
  { id: "sqlite", name: "SQLite", color: "#003B57", available: true },
  { id: "duckdb", name: "DuckDB", color: "#FFF000", available: true },
  {
    id: "postgresql",
    name: "PostgreSQL",
    color: "#336791",
    available: false,
  },
  { id: "mysql", name: "MySQL", color: "#4479A1", available: false },
  {
    id: "sqlserver",
    name: "SQL Server",
    color: "#CC2927",
    available: false,
  },
] as const;

/** SQL execution request sent to the API. */
export interface ExecuteRequest {
  readonly sql: string;
  readonly database: DatabaseId;
  readonly explain?: boolean;
}

/** A single column in a result set. */
export interface ResultColumn {
  readonly name: string;
  readonly type: string;
}

/** A row of query results (values keyed by column name). */
export type ResultRow = Record<string, unknown>;

/** SQL execution response from the API. */
export interface ExecuteResponse {
  readonly columns: readonly ResultColumn[];
  readonly rows: readonly ResultRow[];
  readonly elapsed_ms: number;
  readonly row_count: number;
}

/** Query plan node for visualization. */
export interface PlanNode {
  readonly id: string;
  readonly operator: string;
  readonly estimated_rows: number;
  readonly actual_rows?: number;
  readonly cost: number;
  readonly children: readonly PlanNode[];
  readonly properties: Record<string, string>;
}

/** Explain/optimize response. */
export interface ExplainResponse {
  readonly plan: PlanNode;
  readonly optimized_sql?: string;
  readonly rules_applied: readonly string[];
}

/** SQL dialect translation request. */
export interface TranslateRequest {
  readonly sql: string;
  readonly source: DatabaseId;
  readonly target: DatabaseId;
}

/** SQL dialect translation response. */
export interface TranslateResponse {
  readonly translated_sql: string;
  readonly warnings: readonly string[];
}

/** Isolation level for test sessions. */
export type IsolationLevel =
  | "read_uncommitted"
  | "read_committed"
  | "repeatable_read"
  | "serializable"
  | "snapshot";

/** A step in an isolation test session. */
export interface SessionStep {
  readonly session_id: number;
  readonly step_number: number;
  readonly sql: string;
  readonly result?: ExecuteResponse;
  readonly error?: string;
  readonly locks_held: readonly LockInfo[];
}

/** Lock information for the lock table display. */
export interface LockInfo {
  readonly session_id: number;
  readonly lock_type: "shared" | "exclusive" | "update" | "intent";
  readonly resource: string;
  readonly granted: boolean;
}

/** Isolation test configuration. */
export interface IsolationTestConfig {
  readonly database: DatabaseId;
  readonly isolation_level: IsolationLevel;
  readonly session_count: number;
  readonly setup_sql: string;
  readonly steps: readonly SessionStep[];
}

/** Share link response. */
export interface ShareResponse {
  readonly id: string;
  readonly url: string;
}

/** Application route paths. */
export const ROUTES = {
  home: "/",
  editor: "/editor",
  compare: "/compare",
  isolation: "/isolation",
  translate: "/translate",
  share: "/share/:id",
} as const;

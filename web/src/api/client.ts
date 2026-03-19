/**
 * API client for the RA backend.
 *
 * Uses fetch() to communicate with the Rocket.rs backend.
 * Falls back to mock responses when the backend is unavailable,
 * enabling frontend development without a running server.
 */

import type {
  ExecuteRequest,
  ExecuteResponse,
  ExplainResponse,
  TranslateRequest,
  TranslateResponse,
  ShareResponse,
} from "src/types.ts";

const BASE_URL = "/api";

/** Error from the API with a status code and message. */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const url = `${BASE_URL}${path}`;
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...((options.headers as Record<string, string>) ?? {}),
  };

  try {
    const response = await fetch(url, { ...options, headers });
    if (!response.ok) {
      const body = await response.text();
      throw new ApiError(response.status, body);
    }
    return (await response.json()) as T;
  } catch (error) {
    if (error instanceof ApiError) throw error;
    // Backend unavailable -- return mock data
    return mockResponse<T>(path, options);
  }
}

/** Execute SQL against a database. */
export async function executeSQL(
  req: ExecuteRequest,
): Promise<ExecuteResponse> {
  return request<ExecuteResponse>("/execute", {
    method: "POST",
    body: JSON.stringify(req),
  });
}

/** Get the query plan / explain output. */
export async function explainSQL(
  req: ExecuteRequest,
): Promise<ExplainResponse> {
  return request<ExplainResponse>("/explain", {
    method: "POST",
    body: JSON.stringify({ ...req, explain: true }),
  });
}

/** Translate SQL between dialects. */
export async function translateSQL(
  req: TranslateRequest,
): Promise<TranslateResponse> {
  return request<TranslateResponse>("/translate", {
    method: "POST",
    body: JSON.stringify(req),
  });
}

/** Create a shareable link for the current state. */
export async function createShare(
  state: Record<string, unknown>,
): Promise<ShareResponse> {
  return request<ShareResponse>("/share", {
    method: "POST",
    body: JSON.stringify(state),
  });
}

/** Load a shared state by ID. */
export async function loadShare(
  id: string,
): Promise<Record<string, unknown>> {
  return request<Record<string, unknown>>(`/share/${id}`);
}

/** Visualize a single plan for a SQL query. */
export async function visualizePlan<T>(
  req: { sql: string; hardware_profile?: string },
): Promise<T> {
  return request<T>("/visualize", {
    method: "POST",
    body: JSON.stringify(req),
  });
}

/** Compare plans across multiple optimizers. */
export async function comparePlans<T>(
  req: { sql: string; hardware_profile?: string },
): Promise<T> {
  return request<T>("/compare-plans", {
    method: "POST",
    body: JSON.stringify(req),
  });
}

// ----------------------------------------------------------------
// Mock responses for offline development
// ----------------------------------------------------------------

function mockResponse<T>(path: string, options: RequestInit): T {
  if (path === "/execute") {
    return mockExecuteResponse(options) as T;
  }
  if (path === "/explain") {
    return mockExplainResponse() as T;
  }
  if (path === "/translate") {
    return mockTranslateResponse(options) as T;
  }
  if (path === "/share") {
    return mockShareResponse() as T;
  }
  if (path === "/visualize") {
    return mockVisualizeResponse() as T;
  }
  if (path === "/compare-plans") {
    return mockComparePlansResponse() as T;
  }
  return {} as T;
}

function mockExecuteResponse(options: RequestInit): ExecuteResponse {
  const body = JSON.parse(
    (options.body as string) ?? "{}",
  ) as ExecuteRequest;
  const sql = body.sql?.toLowerCase() ?? "";

  if (sql.includes("select")) {
    return {
      columns: [
        { name: "id", type: "INTEGER" },
        { name: "name", type: "TEXT" },
        { name: "value", type: "REAL" },
      ],
      rows: [
        { id: 1, name: "alpha", value: 10.5 },
        { id: 2, name: "beta", value: 20.3 },
        { id: 3, name: "gamma", value: 30.1 },
      ],
      elapsed_ms: 1.2,
      row_count: 3,
    };
  }

  return {
    columns: [],
    rows: [],
    elapsed_ms: 0.5,
    row_count: 0,
  };
}

function mockExplainResponse(): ExplainResponse {
  return {
    plan: {
      id: "root",
      operator: "Project",
      estimated_rows: 100,
      cost: 150.0,
      children: [
        {
          id: "filter",
          operator: "Filter",
          estimated_rows: 100,
          cost: 50.0,
          children: [
            {
              id: "scan",
              operator: "Scan(users)",
              estimated_rows: 1000,
              cost: 100.0,
              children: [],
              properties: { table: "users" },
            },
          ],
          properties: { predicate: "age > 18" },
        },
      ],
      properties: { columns: "id, name" },
    },
    rules_applied: [
      "filter-pushdown",
      "projection-pruning",
      "constant-folding",
    ],
  };
}

function mockTranslateResponse(
  options: RequestInit,
): TranslateResponse {
  const body = JSON.parse(
    (options.body as string) ?? "{}",
  ) as TranslateRequest;
  return {
    translated_sql: body.sql ?? "",
    warnings: [],
  };
}

function mockShareResponse(): ShareResponse {
  return {
    id: "mock-share-id",
    url: `${window.location.origin}/share/mock-share-id`,
  };
}

function mockPlanNode(
  prefix: string,
  opType: string,
  cost: number,
  rows: number,
  children: unknown[],
  details: { key: string; value: string }[] = [],
): unknown {
  return {
    id: `${prefix}-${String(Math.random()).slice(2, 8)}`,
    operator_type: opType,
    cost,
    rows,
    details,
    children,
    position: { x: 0, y: 0, width: 160, height: 60 },
  };
}

function mockVisualizeResponse(): unknown {
  const scan = mockPlanNode("ra", "SeqScan", 120, 10000, [], [
    { key: "table", value: "users" },
  ]);
  const filter = mockPlanNode("ra", "Filter", 80, 2500, [scan], [
    { key: "predicate", value: "age > 25" },
  ]);
  const project = mockPlanNode(
    "ra",
    "Project",
    10,
    2500,
    [filter],
    [{ key: "columns", value: "*" }],
  );
  return {
    plan: project,
    total_cost: 210,
    rules_applied: [
      "predicate-pushdown",
      "projection-pruning",
    ],
  };
}

function mockComparePlansResponse(): unknown {
  const raScan = mockPlanNode("ra", "SeqScan", 120, 10000, [], [
    { key: "table", value: "users" },
  ]);
  const raFilter = mockPlanNode("ra", "Filter", 80, 2500, [raScan]);
  const raPlan = mockPlanNode("ra", "Project", 10, 2500, [raFilter]);

  const pgScan = mockPlanNode("pg", "Seq Scan", 145, 10000, [], [
    { key: "relation", value: "users" },
  ]);
  const pgFilter = mockPlanNode(
    "pg",
    "Filter",
    95,
    3333,
    [pgScan],
  );
  const pgPlan = mockPlanNode("pg", "Result", 5, 3333, [pgFilter]);

  const myScan = mockPlanNode(
    "mysql",
    "Full Table Scan",
    180,
    10000,
    [],
  );
  const myFilter = mockPlanNode(
    "mysql",
    "Using where",
    110,
    2000,
    [myScan],
  );
  const myPlan = mockPlanNode("mysql", "Query", 5, 2000, [myFilter]);

  const dkScan = mockPlanNode("duck", "SCAN", 95, 10000, []);
  const dkFilter = mockPlanNode(
    "duck",
    "FILTER",
    65,
    2500,
    [dkScan],
  );
  const dkPlan = mockPlanNode(
    "duck",
    "PROJECTION",
    8,
    2500,
    [dkFilter],
  );

  return {
    plans: [
      {
        optimizer: "Ra",
        plan: raPlan,
        total_cost: 210,
        available: true,
      },
      {
        optimizer: "PostgreSQL",
        plan: pgPlan,
        total_cost: 245,
        available: true,
      },
      {
        optimizer: "MySQL",
        plan: myPlan,
        total_cost: 295,
        available: true,
      },
      {
        optimizer: "DuckDB",
        plan: dkPlan,
        total_cost: 168,
        available: true,
      },
    ],
    summary: {
      cheapest: "DuckDB",
      costs: [
        { optimizer: "Ra", total_cost: 210, node_count: 3 },
        { optimizer: "PostgreSQL", total_cost: 245, node_count: 3 },
        { optimizer: "MySQL", total_cost: 295, node_count: 3 },
        { optimizer: "DuckDB", total_cost: 168, node_count: 3 },
      ],
    },
  };
}

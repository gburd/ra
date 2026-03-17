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

/** API client for the ra-web backend. */

const BASE_URL = "/api";

export interface VisualPlanNode {
  id: string;
  operator_type: string;
  cost: number;
  rows: number;
  details: Array<{ key: string; value: string }>;
  children: VisualPlanNode[];
  position: { x: number; y: number; width: number; height: number };
}

export interface VisualizeResponse {
  plan: VisualPlanNode;
  total_cost: number;
  rules_applied: string[];
}

export interface ExecuteResponse {
  columns: string[];
  rows: string[][];
  rows_affected: number;
  engine: string;
}

export interface OptimizeResponse {
  original: unknown;
  optimized: unknown;
  rules_applied: number;
}

export interface RulesResponse {
  count: number;
  rules: string[];
}

export interface ComparePlansResponse {
  plans: Array<{
    optimizer: string;
    plan: VisualPlanNode;
    total_cost: number;
    available: boolean;
  }>;
  summary: {
    cheapest: string;
    costs: Array<{
      optimizer: string;
      total_cost: number;
      node_count: number;
    }>;
  };
}

export interface TranslateResponse {
  from: string;
  to: string;
  original: string;
  translated: string;
}

class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(
  path: string,
  options?: RequestInit,
): Promise<T> {
  const url = `${BASE_URL}${path}`;
  const response = await fetch(url, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });

  if (!response.ok) {
    const body = await response.text();
    throw new ApiError(response.status, body);
  }

  return response.json() as Promise<T>;
}

export function visualize(
  sql: string,
  hardwareProfile?: string,
): Promise<VisualizeResponse> {
  return request<VisualizeResponse>("/visualize", {
    method: "POST",
    body: JSON.stringify({
      sql,
      hardware_profile: hardwareProfile,
    }),
  });
}

export function execute(
  sql: string,
  engine: string,
): Promise<ExecuteResponse> {
  return request<ExecuteResponse>("/execute", {
    method: "POST",
    body: JSON.stringify({ sql, engine }),
  });
}

export function comparePlans(
  sql: string,
  hardwareProfile?: string,
): Promise<ComparePlansResponse> {
  return request<ComparePlansResponse>("/compare-plans", {
    method: "POST",
    body: JSON.stringify({
      sql,
      hardware_profile: hardwareProfile,
    }),
  });
}

export function translate(
  sql: string,
  from: string,
  to: string,
): Promise<TranslateResponse> {
  return request<TranslateResponse>("/translate", {
    method: "POST",
    body: JSON.stringify({ sql, from, to }),
  });
}

export function listRules(): Promise<RulesResponse> {
  return request<RulesResponse>("/rules");
}

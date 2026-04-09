export type Engine =
  | 'postgresql-15'
  | 'postgresql-16'
  | 'postgresql-17'
  | 'mysql-8.0'
  | 'mysql-8.4'
  | 'mariadb-11'
  | 'duckdb'
  | 'sqlite';

export type ExplainMode = 'explain' | 'analyze';

export type VisualizationTab = 'raw' | 'tree' | 'flow' | 'cost' | 'warnings';

export interface EngineConfig {
  id: Engine;
  name: string;
  version: string;
}

export interface PlanNode {
  id: string;
  operation: string;
  relation: string | null;
  cost: { startup: number; total: number };
  rows: number;
  actualTime?: { startup: number; total: number };
  children: string[];
  metadata: Record<string, unknown>;
}

export interface PlanEdge {
  from: string;
  to: string;
  rows: number;
}

export interface ParsedPlan {
  nodes: PlanNode[];
  edges: PlanEdge[];
  rootNodeId: string;
}

export interface OperationCost {
  nodeId: string;
  operation: string;
  cost: number;
  rows: number;
  percentage: number;
}

export interface CostMetrics {
  totalCost: number;
  totalRows: number;
  planDepth: number;
  operationBreakdown: OperationCost[];
  criticalPath?: PlanNode[];
}

export interface Warning {
  severity: 'critical' | 'warning' | 'info';
  type: 'full_table_scan' | 'cartesian_product' | 'missing_index' | 'expensive_sort' | 'inefficient_join' | 'missing_statistics';
  message: string;
  nodeId: string;
  suggestion: string;
}

export interface OutputPanelState {
  id: string;
  engine: Engine;
  output: string | null;
  rawPlan: string | null;
  parsedPlan: ParsedPlan | null;
  costMetrics: CostMetrics | null;
  warnings: Warning[] | null;
  loading: boolean;
  error: string | null;
  activeTab: VisualizationTab;
}

export interface AppState {
  sql: string;
  panels: OutputPanelState[];
  explainMode: ExplainMode;
}

export interface ExplainRequest {
  sql: string;
  engine: string;
  analyze: boolean;
}

export interface ExplainResponse {
  plan: string;
  engine: string;
  analyzed: boolean;
}

export interface ErrorResponse {
  error: string;
}

export interface Schema {
  name: string;
  tables: Table[];
  sampleQueries: SampleQuery[];
}

export interface Table {
  name: string;
  ddl: string;
}

export interface SampleQuery {
  name: string;
  sql: string;
  description: string;
}

export type Engine =
  | 'postgresql-15'
  | 'postgresql-16'
  | 'postgresql-17'
  | 'mysql-8.0'
  | 'mysql-8.4'
  | 'duckdb'
  | 'sqlite';

export type ExplainMode = 'explain' | 'analyze';

export interface EngineConfig {
  id: Engine;
  name: string;
  version: string;
}

export interface OutputPanelState {
  id: string;
  engine: Engine;
  output: string | null;
  loading: boolean;
  error: string | null;
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

import type { Engine, ParsedPlan, CostMetrics, Warning } from '../types';
import { parsePostgresPlan } from './postgresParser';
import { parseMySQLPlan } from './mysqlParser';
import { parseSQLitePlan } from './sqliteParser';
import { parseDuckDBPlan } from './duckdbParser';
import { parseMariaDBPlan } from './mariadbParser';

export interface PlanParser {
  parse(rawPlan: string, engine: Engine): ParsedPlan | null;
  extractCostMetrics(parsedPlan: ParsedPlan): CostMetrics;
  detectWarnings(parsedPlan: ParsedPlan): Warning[];
}

export { parsePostgresPlan, parseMySQLPlan, parseSQLitePlan, parseDuckDBPlan, parseMariaDBPlan };

export function parsePlan(rawPlan: string | null, engine: Engine): ParsedPlan | null {
  if (!rawPlan) {
    return null;
  }

  try {
    switch (engine) {
      case 'postgresql-15':
      case 'postgresql-16':
      case 'postgresql-17':
        return parsePostgresPlan(rawPlan);
      case 'mysql-8.0':
      case 'mysql-8.4':
        return parseMySQLPlan(rawPlan);
      case 'mariadb-11':
        return parseMariaDBPlan(rawPlan);
      case 'sqlite':
        return parseSQLitePlan(rawPlan);
      case 'duckdb':
        return parseDuckDBPlan(rawPlan);
      default:
        return null;
    }
  } catch (error) {
    console.error(`Failed to parse plan for ${engine}:`, error);
    return null;
  }
}

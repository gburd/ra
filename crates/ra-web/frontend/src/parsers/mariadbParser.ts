import { parseMySQLPlan } from './mysqlParser';
import type { ParsedPlan } from '../types';

export function parseMariaDBPlan(rawPlan: string): ParsedPlan | null {
  return parseMySQLPlan(rawPlan);
}

/**
 * Utilities for parsing and formatting EXPLAIN plan output.
 */

export interface CostEstimate {
  startup: number;
  total: number;
  rows: number;
  width: number;
}

export interface ActualStats {
  time: number;
  rows: number;
  loops: number;
}

export interface PlanNode {
  line: string;
  indentLevel: number;
  operation: string | null;
  cost: CostEstimate | null;
  actual: ActualStats | null;
  highlight: boolean;
}

/**
 * Parse cost estimate from EXPLAIN output.
 * Format: (cost=0.00..35.50 rows=2550 width=32)
 */
export function parseCost(line: string): CostEstimate | null {
  const costMatch = /cost=(\d+\.?\d*)\.\.(\d+\.?\d*)/.exec(line);
  const rowsMatch = /rows=(\d+)/.exec(line);
  const widthMatch = /width=(\d+)/.exec(line);

  if (!costMatch) {
    return null;
  }

  return {
    startup: Number.parseFloat(costMatch[1]!),
    total: Number.parseFloat(costMatch[2]!),
    rows: rowsMatch ? Number.parseInt(rowsMatch[1]!, 10) : 0,
    width: widthMatch ? Number.parseInt(widthMatch[1]!, 10) : 0,
  };
}

/**
 * Parse actual stats from EXPLAIN ANALYZE output.
 * Format: (actual time=0.012..0.234 rows=1000 loops=1)
 */
export function parseActual(line: string): ActualStats | null {
  const timeMatch = /actual time=(\d+\.?\d*)\.\.(\d+\.?\d*)/.exec(line);
  const rowsMatch = /rows=(\d+)/.exec(line);
  const loopsMatch = /loops=(\d+)/.exec(line);

  if (!timeMatch) {
    return null;
  }

  return {
    time: Number.parseFloat(timeMatch[2]!),
    rows: rowsMatch ? Number.parseInt(rowsMatch[1]!, 10) : 0,
    loops: loopsMatch ? Number.parseInt(loopsMatch[1]!, 10) : 1,
  };
}

/**
 * Parse timing information (Planning/Execution Time).
 * Format: "Planning Time: 0.123 ms" or "Execution Time: 0.456 ms"
 */
export function parseTimingLine(line: string): { label: string; value: number } | null {
  const match = /(Planning|Execution) Time:\s*(\d+\.?\d*)\s*ms/.exec(line);
  if (!match) {
    return null;
  }

  return {
    label: match[1]!,
    value: Number.parseFloat(match[2]!),
  };
}

/**
 * Format milliseconds to human-readable time.
 */
export function formatTime(ms: number): string {
  if (ms < 1) {
    return `${(ms * 1000).toFixed(0)}µs`;
  }
  if (ms < 1000) {
    return `${ms.toFixed(2)}ms`;
  }
  return `${(ms / 1000).toFixed(2)}s`;
}

/**
 * Format large numbers with thousands separators.
 */
export function formatNumber(num: number): string {
  return num.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}

/**
 * Calculate indent level based on leading spaces.
 */
export function getIndentLevel(line: string): number {
  const match = /^(\s*)/.exec(line);
  if (!match) {
    return 0;
  }
  return Math.floor(match[1]!.length / 2);
}

/**
 * Extract operation name from plan line.
 * Examples: "Seq Scan", "Hash Join", "Sort", "Aggregate"
 */
export function extractOperation(line: string): string | null {
  const trimmed = line.trim();

  // Common operations
  const operations = [
    'Seq Scan',
    'Index Scan',
    'Index Only Scan',
    'Bitmap Heap Scan',
    'Bitmap Index Scan',
    'Hash Join',
    'Nested Loop',
    'Merge Join',
    'Hash',
    'Sort',
    'Aggregate',
    'Group',
    'Filter',
    'Limit',
    'Subquery Scan',
  ];

  for (const op of operations) {
    if (trimmed.startsWith(op)) {
      return op;
    }
  }

  return null;
}

/**
 * Parse plan output into structured nodes.
 */
export function parsePlan(planText: string): PlanNode[] {
  const lines = planText.split('\n');
  const nodes: PlanNode[] = [];

  for (const line of lines) {
    if (line.trim().length === 0) {
      continue;
    }

    nodes.push({
      line,
      indentLevel: getIndentLevel(line),
      operation: extractOperation(line),
      cost: parseCost(line),
      actual: parseActual(line),
      highlight: false,
    });
  }

  return nodes;
}

/**
 * Find all matches for a search term in the plan text.
 */
export function findMatches(
  planText: string,
  searchTerm: string
): Array<{ lineIndex: number; charIndex: number }> {
  if (!searchTerm) {
    return [];
  }

  const matches: Array<{ lineIndex: number; charIndex: number }> = [];
  const lines = planText.split('\n');
  const regex = new RegExp(searchTerm, 'gi');

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
    const line = lines[lineIndex];
    if (!line) {
      continue;
    }

    let match: RegExpExecArray | null;
    while ((match = regex.exec(line)) !== null) {
      matches.push({ lineIndex, charIndex: match.index });
    }
  }

  return matches;
}

import type { ParsedPlan, Warning, PlanNode } from '../types';

function detectFullTableScans(nodes: PlanNode[]): Warning[] {
  const warnings: Warning[] = [];

  for (const node of nodes) {
    const isFullTableScan =
      node.operation.toLowerCase().includes('seq scan') ||
      node.operation.toLowerCase().includes('table scan') ||
      (node.operation.toLowerCase().includes('scan') &&
        !node.operation.toLowerCase().includes('index'));

    const hasLargeRowCount = node.rows > 1000;

    if (isFullTableScan && hasLargeRowCount) {
      warnings.push({
        severity: 'warning',
        type: 'full_table_scan',
        message: `Full table scan on ${node.relation ?? 'table'} (${node.rows} rows)`,
        nodeId: node.id,
        suggestion: `Consider adding an index on frequently filtered columns for ${node.relation ?? 'this table'}.`,
      });
    }
  }

  return warnings;
}

function detectCartesianProducts(nodes: PlanNode[]): Warning[] {
  const warnings: Warning[] = [];

  for (const node of nodes) {
    const isJoin =
      node.operation.toLowerCase().includes('join') ||
      node.operation.toLowerCase().includes('nested loop');

    const hasHighRowEstimate = node.rows > 100000;

    const metadata = node.metadata as Record<string, unknown>;
    const hasJoinCondition =
      metadata &&
      (metadata['Join Filter'] !== undefined ||
        metadata['Hash Cond'] !== undefined ||
        metadata['Index Cond'] !== undefined);

    if (isJoin && hasHighRowEstimate && !hasJoinCondition) {
      warnings.push({
        severity: 'critical',
        type: 'cartesian_product',
        message: `Potential cartesian product detected (${node.rows} estimated rows)`,
        nodeId: node.id,
        suggestion: 'Add explicit JOIN conditions to avoid cartesian product.',
      });
    }
  }

  return warnings;
}

function detectInefficientJoins(nodes: PlanNode[]): Warning[] {
  const warnings: Warning[] = [];

  for (const node of nodes) {
    const isNestedLoop = node.operation.toLowerCase().includes('nested loop');
    const hasLargeRowCount = node.rows > 10000;

    if (isNestedLoop && hasLargeRowCount) {
      warnings.push({
        severity: 'warning',
        type: 'inefficient_join',
        message: `Nested loop join with high row count (${node.rows} rows)`,
        nodeId: node.id,
        suggestion: 'Consider hash join or merge join instead. Add indexes on join columns.',
      });
    }
  }

  return warnings;
}

function detectExpensiveSorts(nodes: PlanNode[]): Warning[] {
  const warnings: Warning[] = [];

  for (const node of nodes) {
    const isSort =
      node.operation.toLowerCase().includes('sort') ||
      node.operation.toLowerCase().includes('filesort');

    const hasLargeRowCount = node.rows > 100000;

    if (isSort && hasLargeRowCount) {
      warnings.push({
        severity: 'warning',
        type: 'expensive_sort',
        message: `Expensive sort operation on ${node.rows} rows`,
        nodeId: node.id,
        suggestion: 'Consider adding an index to avoid sorting, or limit result set size.',
      });
    }
  }

  return warnings;
}

function detectMissingStatistics(nodes: PlanNode[]): Warning[] {
  const warnings: Warning[] = [];

  for (const node of nodes) {
    const hasZeroCost = node.cost.total === 0 && node.operation !== 'Query Block';
    const hasZeroRows = node.rows === 0 && node.children.length > 0;

    if (hasZeroCost || hasZeroRows) {
      warnings.push({
        severity: 'info',
        type: 'missing_statistics',
        message: `Potentially missing or outdated statistics for ${node.relation ?? 'operation'}`,
        nodeId: node.id,
        suggestion: 'Run ANALYZE or UPDATE STATISTICS on this table.',
      });
    }
  }

  return warnings;
}

function detectMissingIndexes(nodes: PlanNode[]): Warning[] {
  const warnings: Warning[] = [];

  for (const node of nodes) {
    const metadata = node.metadata as Record<string, unknown>;

    const hasFilter = metadata && (metadata['Filter'] !== undefined || metadata['WHERE'] !== undefined);

    const isFullScan =
      node.operation.toLowerCase().includes('seq scan') ||
      node.operation.toLowerCase().includes('table scan');

    const hasModerateRows = node.rows > 100 && node.rows < 1000000;

    if (hasFilter && isFullScan && hasModerateRows) {
      warnings.push({
        severity: 'info',
        type: 'missing_index',
        message: `Filtered scan on ${node.relation ?? 'table'} without index`,
        nodeId: node.id,
        suggestion: `Add an index on the filtered columns for ${node.relation ?? 'this table'}.`,
      });
    }
  }

  return warnings;
}

export function detectWarnings(parsedPlan: ParsedPlan): Warning[] {
  const allWarnings: Warning[] = [];

  allWarnings.push(...detectFullTableScans(parsedPlan.nodes));
  allWarnings.push(...detectCartesianProducts(parsedPlan.nodes));
  allWarnings.push(...detectInefficientJoins(parsedPlan.nodes));
  allWarnings.push(...detectExpensiveSorts(parsedPlan.nodes));
  allWarnings.push(...detectMissingStatistics(parsedPlan.nodes));
  allWarnings.push(...detectMissingIndexes(parsedPlan.nodes));

  return allWarnings;
}

export function extractCostMetrics(parsedPlan: ParsedPlan) {
  const nodes = parsedPlan.nodes;

  let totalCost = 0;
  let totalRows = 0;
  let maxDepth = 0;

  const operationCosts: Map<string, { cost: number; rows: number; count: number }> = new Map();

  function calculateDepth(nodeId: string, depth: number): number {
    const node = nodes.find((n) => n.id === nodeId);
    if (!node) {
      return depth;
    }

    maxDepth = Math.max(maxDepth, depth);

    const opKey = node.operation;
    const existing = operationCosts.get(opKey) ?? { cost: 0, rows: 0, count: 0 };
    operationCosts.set(opKey, {
      cost: existing.cost + node.cost.total,
      rows: existing.rows + node.rows,
      count: existing.count + 1,
    });

    let childMaxDepth = depth;
    for (const childId of node.children) {
      const childDepth = calculateDepth(childId, depth + 1);
      childMaxDepth = Math.max(childMaxDepth, childDepth);
    }

    return childMaxDepth;
  }

  calculateDepth(parsedPlan.rootNodeId, 0);

  const rootNode = nodes.find((n) => n.id === parsedPlan.rootNodeId);
  if (rootNode) {
    totalCost = rootNode.cost.total;
    totalRows = rootNode.rows;
  }

  const operationBreakdown = Array.from(operationCosts.entries()).map(([operation, data]) => ({
    nodeId: '',
    operation,
    cost: data.cost,
    rows: data.rows,
    percentage: totalCost > 0 ? (data.cost / totalCost) * 100 : 0,
  }));

  operationBreakdown.sort((a, b) => b.cost - a.cost);

  return {
    totalCost,
    totalRows,
    planDepth: maxDepth + 1,
    operationBreakdown,
  };
}

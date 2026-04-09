import type { ParsedPlan, PlanNode, PlanEdge } from '../types';

interface MySQLTableInfo {
  table_name: string;
  access_type: string;
  possible_keys?: string[];
  key?: string;
  rows_examined_per_scan?: number;
  cost_info?: {
    read_cost: string;
    eval_cost: string;
    prefix_cost: string;
    data_read_per_join: string;
  };
  [key: string]: unknown;
}


let nodeIdCounter = 0;

function generateNodeId(): string {
  return `node_${nodeIdCounter++}`;
}

function parseTable(table: MySQLTableInfo, nodes: PlanNode[], edges: PlanEdge[], parentId: string | null): string {
  const nodeId = generateNodeId();

  const cost = table.cost_info
    ? parseFloat(table.cost_info.read_cost) + parseFloat(table.cost_info.eval_cost)
    : 0;

  const node: PlanNode = {
    id: nodeId,
    operation: table.access_type === 'ALL' ? 'Seq Scan' : 'Index Scan',
    relation: table.table_name,
    cost: {
      startup: 0,
      total: cost,
    },
    rows: table.rows_examined_per_scan ?? 0,
    children: [],
    metadata: table,
  };

  nodes.push(node);

  if (parentId) {
    edges.push({
      from: parentId,
      to: nodeId,
      rows: node.rows,
    });
  }

  return nodeId;
}

export function parseMySQLPlan(rawPlan: string): ParsedPlan | null {
  try {
    nodeIdCounter = 0;

    const parsed = JSON.parse(rawPlan);
    const nodes: PlanNode[] = [];
    const edges: PlanEdge[] = [];

    let rootNodeId = generateNodeId();

    const queryBlock = parsed.query_block ?? parsed;

    const cost = queryBlock.cost_info
      ? parseFloat(queryBlock.cost_info.query_cost)
      : 0;

    const rootNode: PlanNode = {
      id: rootNodeId,
      operation: 'Query Block',
      relation: null,
      cost: {
        startup: 0,
        total: cost,
      },
      rows: 0,
      children: [],
      metadata: queryBlock,
    };

    nodes.push(rootNode);

    if (queryBlock.table) {
      const tables = Array.isArray(queryBlock.table) ? queryBlock.table : [queryBlock.table];
      for (const table of tables) {
        const childId = parseTable(table, nodes, edges, rootNodeId);
        rootNode.children.push(childId);
      }
    }

    if (queryBlock.nested_loop && Array.isArray(queryBlock.nested_loop)) {
      for (const loop of queryBlock.nested_loop) {
        if (loop.table) {
          const childId = parseTable(loop.table, nodes, edges, rootNodeId);
          rootNode.children.push(childId);
        }
      }
    }

    return {
      nodes,
      edges,
      rootNodeId,
    };
  } catch (error) {
    console.error('Failed to parse MySQL plan:', error);
    return null;
  }
}

import type { ParsedPlan, PlanNode, PlanEdge } from '../types';

interface PostgresPlanNode {
  'Node Type': string;
  'Relation Name'?: string;
  'Startup Cost': number;
  'Total Cost': number;
  'Plan Rows': number;
  'Actual Startup Time'?: number;
  'Actual Total Time'?: number;
  'Plans'?: PostgresPlanNode[];
  [key: string]: unknown;
}

let nodeIdCounter = 0;

function generateNodeId(): string {
  return `node_${nodeIdCounter++}`;
}

function parseNode(pgNode: PostgresPlanNode, parentId: string | null, nodes: PlanNode[], edges: PlanEdge[]): string {
  const nodeId = generateNodeId();

  const hasActualTime = pgNode['Actual Startup Time'] !== undefined && pgNode['Actual Total Time'] !== undefined;

  const node: PlanNode = {
    id: nodeId,
    operation: pgNode['Node Type'],
    relation: pgNode['Relation Name'] ?? null,
    cost: {
      startup: pgNode['Startup Cost'],
      total: pgNode['Total Cost'],
    },
    rows: pgNode['Plan Rows'],
    ...(hasActualTime && {
      actualTime: {
        startup: pgNode['Actual Startup Time']!,
        total: pgNode['Actual Total Time']!,
      },
    }),
    children: [],
    metadata: pgNode,
  };

  nodes.push(node);

  if (parentId) {
    edges.push({
      from: parentId,
      to: nodeId,
      rows: node.rows,
    });
  }

  if (pgNode.Plans) {
    for (const childPlan of pgNode.Plans) {
      const childId = parseNode(childPlan, nodeId, nodes, edges);
      node.children.push(childId);
    }
  }

  return nodeId;
}

export function parsePostgresPlan(rawPlan: string): ParsedPlan | null {
  try {
    nodeIdCounter = 0;

    const parsed = JSON.parse(rawPlan);

    let planData = parsed;
    if (Array.isArray(parsed)) {
      planData = parsed[0];
    }

    if (planData.Plan) {
      planData = planData.Plan;
    }

    const nodes: PlanNode[] = [];
    const edges: PlanEdge[] = [];

    const rootNodeId = parseNode(planData, null, nodes, edges);

    return {
      nodes,
      edges,
      rootNodeId,
    };
  } catch (error) {
    console.error('Failed to parse PostgreSQL plan:', error);
    return null;
  }
}

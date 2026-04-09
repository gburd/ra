import type { ParsedPlan, PlanNode, PlanEdge } from '../types';

let nodeIdCounter = 0;

function generateNodeId(): string {
  return `node_${nodeIdCounter++}`;
}

export function parseSQLitePlan(rawPlan: string): ParsedPlan | null {
  try {
    nodeIdCounter = 0;

    const lines = rawPlan.trim().split('\n');
    const nodes: PlanNode[] = [];
    const edges: PlanEdge[] = [];

    const stack: Array<{ node: PlanNode; depth: number }> = [];
    let rootNodeId: string | null = null;

    for (const line of lines) {
      if (line.startsWith('--')) {
        continue;
      }

      const match = line.match(/^(\|--|\`--)?(\s*)(\d+)\s+(\d+)\s+(\d+)\s+(.+)$/);
      if (!match) {
        continue;
      }

      const prefix = match[1];
      const spaces = match[2] ?? '';
      const id = match[3]!;
      const parent = match[4]!;
      const detail = match[6]!;
      const depth = (prefix?.length ?? 0) + spaces.length;

      const nodeId = generateNodeId();
      const operation = detail.trim();

      const relationMatch = operation.match(/SCAN TABLE (\w+)|SEARCH TABLE (\w+)|SCAN (\w+)/);
      const relation = relationMatch?.[1] ?? relationMatch?.[2] ?? relationMatch?.[3] ?? null;

      const node: PlanNode = {
        id: nodeId,
        operation,
        relation,
        cost: { startup: 0, total: 0 },
        rows: 0,
        children: [],
        metadata: { id, parent, detail },
      };

      nodes.push(node);

      while (stack.length > 0 && stack[stack.length - 1]!.depth >= depth) {
        stack.pop();
      }

      if (stack.length > 0) {
        const parentEntry = stack[stack.length - 1];
        if (parentEntry) {
          parentEntry.node.children.push(nodeId);
          edges.push({
            from: parentEntry.node.id,
            to: nodeId,
            rows: 0,
          });
        }
      } else {
        rootNodeId = nodeId;
      }

      stack.push({ node, depth });
    }

    if (!rootNodeId) {
      return null;
    }

    return {
      nodes,
      edges,
      rootNodeId,
    };
  } catch (error) {
    console.error('Failed to parse SQLite plan:', error);
    return null;
  }
}

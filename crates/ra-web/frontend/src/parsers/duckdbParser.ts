import type { ParsedPlan, PlanNode, PlanEdge } from '../types';

let nodeIdCounter = 0;

function generateNodeId(): string {
  return `node_${nodeIdCounter++}`;
}

export function parseDuckDBPlan(rawPlan: string): ParsedPlan | null {
  try {
    nodeIdCounter = 0;

    const lines = rawPlan.trim().split('\n');
    const nodes: PlanNode[] = [];
    const edges: PlanEdge[] = [];

    const stack: Array<{ node: PlanNode; depth: number }> = [];
    let rootNodeId: string | null = null;

    for (const line of lines) {
      if (line.trim().length === 0 || line.startsWith('┌') || line.startsWith('└') || line.startsWith('─')) {
        continue;
      }

      const leadingSpaces = line.search(/\S/);
      const depth = leadingSpaces;

      let cleanLine = line.trim();
      cleanLine = cleanLine.replace(/^[│├└─\s]+/, '');

      if (!cleanLine) {
        continue;
      }

      const nodeId = generateNodeId();

      const relationMatch = cleanLine.match(/\[(.+?)\]/);
      const relation = relationMatch?.[1] ?? null;

      let operation = cleanLine.replace(/\[.+?\]/, '').trim();
      if (!operation) {
        operation = cleanLine;
      }

      const rowsMatch = cleanLine.match(/(\d+)\s+Rows/i);
      const rows = rowsMatch?.[1] ? parseInt(rowsMatch[1], 10) : 0;

      const node: PlanNode = {
        id: nodeId,
        operation,
        relation,
        cost: { startup: 0, total: 0 },
        rows,
        children: [],
        metadata: { raw: line },
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
            rows: node.rows,
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
    console.error('Failed to parse DuckDB plan:', error);
    return null;
  }
}

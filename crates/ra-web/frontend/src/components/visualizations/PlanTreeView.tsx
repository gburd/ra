import { useEffect, useRef } from 'react';
import { Box, Paper } from '@mui/material';
import * as d3 from 'd3';
import type { ParsedPlan, PlanNode } from '../../types';

interface PlanTreeViewProps {
  parsedPlan: ParsedPlan;
  highlightedNodeId: string | undefined;
  onNodeClick: ((nodeId: string) => void) | undefined;
}

interface TreeNode extends d3.HierarchyPointNode<PlanNode> {
  _children?: TreeNode[];
}

const OPERATION_COLORS: Record<string, string> = {
  'Seq Scan': '#60A5FA',
  'Index Scan': '#34D399',
  'Index Only Scan': '#34D399',
  'Bitmap Heap Scan': '#34D399',
  'Hash Join': '#F87171',
  'Merge Join': '#F87171',
  'Nested Loop': '#F87171',
  'Aggregate': '#C084FC',
  'HashAggregate': '#C084FC',
  'Sort': '#FB923C',
  'Limit': '#FCD34D',
};

function getOperationColor(operation: string): string {
  for (const [key, color] of Object.entries(OPERATION_COLORS)) {
    if (operation.includes(key)) {
      return color;
    }
  }
  return '#94A3B8';
}

export function PlanTreeView({ parsedPlan, highlightedNodeId, onNodeClick }: PlanTreeViewProps) {
  const svgRef = useRef<SVGSVGElement>(null);

  useEffect(() => {
    if (!svgRef.current || !parsedPlan) {
      return;
    }

    const width = svgRef.current.clientWidth;

    const svg = d3.select(svgRef.current);
    svg.selectAll('*').remove();

    const g = svg
      .append('g')
      .attr('transform', `translate(${width / 2}, 40)`);

    const zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.1, 3])
      .on('zoom', (event) => {
        g.attr('transform', event.transform);
      });

    svg.call(zoom);

    function buildHierarchy(nodeId: string, nodes: PlanNode[]): d3.HierarchyNode<PlanNode> {
      const node = nodes.find((n) => n.id === nodeId);
      if (!node) {
        throw new Error(`Node ${nodeId} not found`);
      }

      const childHierarchies = node.children
        .map((childId) => buildHierarchy(childId, nodes))
        .filter(Boolean);

      return d3.hierarchy(node, () => childHierarchies.length > 0 ? childHierarchies.map(h => h.data) : null);
    }

    const rootHierarchy = buildHierarchy(parsedPlan.rootNodeId, parsedPlan.nodes);

    const treeLayout = d3.tree<PlanNode>().nodeSize([200, 100]);
    const root = treeLayout(rootHierarchy) as TreeNode;

    const links = root.links();
    const nodes = root.descendants();

    g.selectAll('.link')
      .data(links)
      .join('path')
      .attr('class', 'link')
      .attr('d', d3.linkVertical<d3.HierarchyPointLink<PlanNode>, d3.HierarchyPointNode<PlanNode>>()
        .x((d) => d.x)
        .y((d) => d.y)
      )
      .attr('fill', 'none')
      .attr('stroke', '#64748B')
      .attr('stroke-width', 2)
      .attr('stroke-opacity', 0.4);

    const nodeGroup = g.selectAll('.node')
      .data(nodes)
      .join('g')
      .attr('class', 'node')
      .attr('transform', (d) => `translate(${d.x}, ${d.y})`)
      .style('cursor', 'pointer')
      .on('click', (event, d) => {
        event.stopPropagation();
        if (onNodeClick) {
          onNodeClick(d.data.id);
        }
      });

    nodeGroup
      .append('circle')
      .attr('r', 8)
      .attr('fill', (d) => getOperationColor(d.data.operation))
      .attr('stroke', (d) => d.data.id === highlightedNodeId ? '#FDE047' : '#1E293B')
      .attr('stroke-width', (d) => d.data.id === highlightedNodeId ? 4 : 2);

    nodeGroup
      .append('rect')
      .attr('x', -80)
      .attr('y', 12)
      .attr('width', 160)
      .attr('height', 50)
      .attr('rx', 4)
      .attr('fill', (d) => d.data.id === highlightedNodeId ? '#FEF3C7' : '#1E293B')
      .attr('stroke', (d) => getOperationColor(d.data.operation))
      .attr('stroke-width', 2);

    nodeGroup
      .append('text')
      .attr('y', 32)
      .attr('text-anchor', 'middle')
      .attr('fill', (d) => d.data.id === highlightedNodeId ? '#000' : '#F1F5F9')
      .attr('font-size', 12)
      .attr('font-weight', 'bold')
      .text((d) => d.data.operation.length > 20 ? d.data.operation.substring(0, 18) + '...' : d.data.operation);

    nodeGroup
      .append('text')
      .attr('y', 48)
      .attr('text-anchor', 'middle')
      .attr('fill', (d) => d.data.id === highlightedNodeId ? '#000' : '#94A3B8')
      .attr('font-size', 10)
      .text((d) => {
        const parts: string[] = [];
        if (d.data.relation) {
          parts.push(d.data.relation);
        }
        if (d.data.rows > 0) {
          parts.push(`${d.data.rows} rows`);
        }
        return parts.join(' • ');
      });

    nodeGroup.append('title').text((d) => {
      const lines = [
        `Operation: ${d.data.operation}`,
        d.data.relation ? `Relation: ${d.data.relation}` : null,
        `Cost: ${d.data.cost.startup.toFixed(2)} .. ${d.data.cost.total.toFixed(2)}`,
        `Rows: ${d.data.rows}`,
      ].filter(Boolean);
      return lines.join('\n');
    });

  }, [parsedPlan, highlightedNodeId, onNodeClick]);

  return (
    <Box sx={{ width: '100%', height: '100%', position: 'relative' }}>
      <Paper
        elevation={0}
        sx={{
          width: '100%',
          height: '100%',
          bgcolor: '#0F172A',
          overflow: 'hidden',
        }}
      >
        <svg
          ref={svgRef}
          style={{
            width: '100%',
            height: '100%',
            display: 'block',
          }}
        />
      </Paper>
    </Box>
  );
}

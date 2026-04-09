import { useEffect, useMemo } from 'react';
import {
  ReactFlow,
  type Node,
  type Edge,
  Background,
  Controls,
  MiniMap,
  useNodesState,
  useEdgesState,
  MarkerType,
} from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import { Box } from '@mui/material';
import dagre from 'dagre';
import type { ParsedPlan, PlanNode as AppPlanNode } from '../../types';

interface PlanFlowViewProps {
  parsedPlan: ParsedPlan;
  highlightedNodeId: string | undefined;
  onNodeClick: ((nodeId: string) => void) | undefined;
}

function getNodeType(operation: string): 'input' | 'default' | 'output' {
  const op = operation.toLowerCase();

  if (op.includes('scan') || op.includes('index')) {
    return 'input';
  }

  if (op.includes('result') || op.includes('return')) {
    return 'output';
  }

  return 'default';
}

function getNodeColor(operation: string): string {
  const op = operation.toLowerCase();

  if (op.includes('scan')) return '#60A5FA';
  if (op.includes('index')) return '#34D399';
  if (op.includes('join')) return '#F87171';
  if (op.includes('aggregate') || op.includes('group')) return '#C084FC';
  if (op.includes('sort')) return '#FB923C';
  if (op.includes('filter') || op.includes('where')) return '#FCD34D';

  return '#94A3B8';
}

function layoutNodes(nodes: Node[], edges: Edge[]): Node[] {
  const dagreGraph = new dagre.graphlib.Graph();
  dagreGraph.setDefaultEdgeLabel(() => ({}));
  dagreGraph.setGraph({ rankdir: 'LR', nodesep: 100, ranksep: 150 });

  nodes.forEach((node) => {
    dagreGraph.setNode(node.id, { width: 200, height: 80 });
  });

  edges.forEach((edge) => {
    dagreGraph.setEdge(edge.source, edge.target);
  });

  dagre.layout(dagreGraph);

  return nodes.map((node) => {
    const nodeWithPosition = dagreGraph.node(node.id);
    return {
      ...node,
      position: {
        x: nodeWithPosition.x - 100,
        y: nodeWithPosition.y - 40,
      },
    };
  });
}

export function PlanFlowView({ parsedPlan, highlightedNodeId, onNodeClick }: PlanFlowViewProps) {
  const initialNodes: Node[] = useMemo(() => {
    return parsedPlan.nodes.map((node: AppPlanNode) => ({
      id: node.id,
      type: getNodeType(node.operation),
      position: { x: 0, y: 0 },
      data: {
        label: (
          <div style={{ textAlign: 'center' }}>
            <div style={{ fontWeight: 'bold', fontSize: '12px' }}>
              {node.operation}
            </div>
            {node.relation && (
              <div style={{ fontSize: '10px', color: '#94A3B8' }}>
                {node.relation}
              </div>
            )}
            <div style={{ fontSize: '10px', color: '#64748B' }}>
              {node.rows > 0 ? `${node.rows} rows` : ''}
            </div>
          </div>
        ),
      },
      style: {
        background: getNodeColor(node.operation),
        color: '#FFF',
        border: node.id === highlightedNodeId ? '3px solid #FDE047' : '2px solid #1E293B',
        borderRadius: '8px',
        padding: '10px',
        width: 200,
        fontSize: '12px',
      },
    }));
  }, [parsedPlan, highlightedNodeId]);

  const initialEdges: Edge[] = useMemo(() => {
    return parsedPlan.edges.map((edge) => ({
      id: `${edge.from}-${edge.to}`,
      source: edge.from,
      target: edge.to,
      animated: false,
      style: {
        stroke: '#64748B',
        strokeWidth: Math.max(1, Math.min(edge.rows / 1000, 8)),
      },
      markerEnd: {
        type: MarkerType.ArrowClosed,
        color: '#64748B',
      },
      label: edge.rows > 0 ? `${edge.rows} rows` : undefined,
      labelStyle: { fill: '#94A3B8', fontSize: 10 },
      labelBgStyle: { fill: '#1E293B', fillOpacity: 0.8 },
    }));
  }, [parsedPlan]);

  const layoutedNodes = useMemo(() => {
    return layoutNodes(initialNodes, initialEdges);
  }, [initialNodes, initialEdges]);

  const [nodes, setNodes, onNodesChange] = useNodesState(layoutedNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges);

  useEffect(() => {
    setNodes(layoutedNodes);
  }, [layoutedNodes, setNodes]);

  useEffect(() => {
    setEdges(initialEdges);
  }, [initialEdges, setEdges]);

  const handleNodeClick = (_event: React.MouseEvent, node: Node) => {
    if (onNodeClick) {
      onNodeClick(node.id);
    }
  };

  return (
    <Box sx={{ width: '100%', height: '100%', bgcolor: '#0F172A' }}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodeClick={handleNodeClick}
        fitView
        minZoom={0.1}
        maxZoom={2}
        style={{ background: '#0F172A' }}
      >
        <Background color="#1E293B" gap={16} />
        <Controls />
        <MiniMap
          nodeColor={(node) => {
            const style = node.style as Record<string, unknown>;
            return (style?.['background'] as string) ?? '#94A3B8';
          }}
          maskColor="rgba(15, 23, 42, 0.7)"
        />
      </ReactFlow>
    </Box>
  );
}

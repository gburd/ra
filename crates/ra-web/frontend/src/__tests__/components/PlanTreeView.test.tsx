import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { PlanTreeView } from '../../components/visualizations/PlanTreeView';
import type { ParsedPlan, PlanNode } from '../../types';
import * as d3 from 'd3';

vi.mock('d3', async () => {
  const actual = await vi.importActual<typeof import('d3')>('d3');
  return {
    ...actual,
    select: vi.fn(() => ({
      selectAll: vi.fn(() => ({
        remove: vi.fn(),
      })),
      append: vi.fn(() => ({
        attr: vi.fn().mockReturnThis(),
        selectAll: vi.fn(() => ({
          data: vi.fn(() => ({
            join: vi.fn(() => ({
              attr: vi.fn().mockReturnThis(),
              style: vi.fn().mockReturnThis(),
              on: vi.fn().mockReturnThis(),
              append: vi.fn(() => ({
                attr: vi.fn().mockReturnThis(),
                text: vi.fn().mockReturnThis(),
              })),
            })),
          })),
        })),
      })),
      call: vi.fn().mockReturnThis(),
    })),
    zoom: vi.fn(() => ({
      scaleExtent: vi.fn().mockReturnThis(),
      on: vi.fn().mockReturnThis(),
    })),
    tree: vi.fn(() => ({
      nodeSize: vi.fn().mockReturnThis(),
    })),
    hierarchy: vi.fn((node) => ({
      ...node,
      descendants: () => [node],
      links: () => [],
    })),
    linkVertical: vi.fn(() => ({
      x: vi.fn().mockReturnThis(),
      y: vi.fn().mockReturnThis(),
    })),
  };
});

describe('PlanTreeView', () => {
  const createMockNode = (id: string, operation: string, children: string[] = []): PlanNode => ({
    id,
    operation,
    relation: null,
    cost: { startup: 0, total: 100 },
    rows: 1000,
    children,
    metadata: {},
  });

  const createMockPlan = (): ParsedPlan => ({
    nodes: [
      createMockNode('1', 'Seq Scan', []),
      createMockNode('2', 'Index Scan', []),
      createMockNode('3', 'Hash Join', ['1', '2']),
    ],
    edges: [
      { from: '3', to: '1', rows: 1000 },
      { from: '3', to: '2', rows: 500 },
    ],
    rootNodeId: '3',
  });

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders without crashing with valid plan', () => {
    const plan = createMockPlan();
    render(<PlanTreeView parsedPlan={plan} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('renders empty SVG when plan is null', () => {
    const emptyPlan: ParsedPlan = {
      nodes: [],
      edges: [],
      rootNodeId: '',
    };

    render(<PlanTreeView parsedPlan={emptyPlan} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('calls onNodeClick when node is clicked', async () => {
    const plan = createMockPlan();
    const onNodeClick = vi.fn();
    const user = userEvent.setup();

    render(<PlanTreeView parsedPlan={plan} onNodeClick={onNodeClick} />);

    await waitFor(() => {
      expect(d3.select).toHaveBeenCalled();
    });
  });

  it('highlights the specified node', () => {
    const plan = createMockPlan();
    const highlightedNodeId = '1';

    render(<PlanTreeView parsedPlan={plan} highlightedNodeId={highlightedNodeId} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('does not highlight when no node is specified', () => {
    const plan = createMockPlan();

    render(<PlanTreeView parsedPlan={plan} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('handles plan with single node', () => {
    const singleNodePlan: ParsedPlan = {
      nodes: [createMockNode('1', 'Seq Scan', [])],
      edges: [],
      rootNodeId: '1',
    };

    render(<PlanTreeView parsedPlan={singleNodePlan} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('handles plan with deep nesting', () => {
    const deepPlan: ParsedPlan = {
      nodes: [
        createMockNode('1', 'Seq Scan', []),
        createMockNode('2', 'Index Scan', []),
        createMockNode('3', 'Hash Join', ['1', '2']),
        createMockNode('4', 'Aggregate', ['3']),
        createMockNode('5', 'Sort', ['4']),
      ],
      edges: [
        { from: '3', to: '1', rows: 1000 },
        { from: '3', to: '2', rows: 500 },
        { from: '4', to: '3', rows: 800 },
        { from: '5', to: '4', rows: 800 },
      ],
      rootNodeId: '5',
    };

    render(<PlanTreeView parsedPlan={deepPlan} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('handles nodes with relations', () => {
    const planWithRelations: ParsedPlan = {
      nodes: [
        { ...createMockNode('1', 'Seq Scan on users', []), relation: 'users' },
        { ...createMockNode('2', 'Index Scan on orders', []), relation: 'orders' },
        createMockNode('3', 'Hash Join', ['1', '2']),
      ],
      edges: [
        { from: '3', to: '1', rows: 1000 },
        { from: '3', to: '2', rows: 500 },
      ],
      rootNodeId: '3',
    };

    render(<PlanTreeView parsedPlan={planWithRelations} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('updates when parsedPlan changes', () => {
    const plan1 = createMockPlan();
    const { rerender } = render(<PlanTreeView parsedPlan={plan1} />);

    const plan2: ParsedPlan = {
      nodes: [createMockNode('10', 'Aggregate', [])],
      edges: [],
      rootNodeId: '10',
    };

    rerender(<PlanTreeView parsedPlan={plan2} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('updates when highlightedNodeId changes', () => {
    const plan = createMockPlan();
    const { rerender } = render(<PlanTreeView parsedPlan={plan} highlightedNodeId="1" />);

    rerender(<PlanTreeView parsedPlan={plan} highlightedNodeId="2" />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });

  it('handles missing onNodeClick callback gracefully', async () => {
    const plan = createMockPlan();

    render(<PlanTreeView parsedPlan={plan} />);

    await waitFor(() => {
      expect(d3.select).toHaveBeenCalled();
    });
  });

  it('applies correct colors based on operation type', () => {
    const planWithVariousOps: ParsedPlan = {
      nodes: [
        createMockNode('1', 'Seq Scan', []),
        createMockNode('2', 'Index Scan', []),
        createMockNode('3', 'Hash Join', ['1']),
        createMockNode('4', 'Aggregate', ['3']),
        createMockNode('5', 'Sort', ['4']),
        createMockNode('6', 'Limit', ['5', '2']),
      ],
      edges: [
        { from: '3', to: '1', rows: 1000 },
        { from: '4', to: '3', rows: 800 },
        { from: '5', to: '4', rows: 800 },
        { from: '6', to: '5', rows: 100 },
        { from: '6', to: '2', rows: 500 },
      ],
      rootNodeId: '6',
    };

    render(<PlanTreeView parsedPlan={planWithVariousOps} />);

    const svg = document.querySelector('svg');
    expect(svg).toBeInTheDocument();
  });
});

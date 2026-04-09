/**
 * Example usage of the DiffView component
 *
 * This file demonstrates how to integrate the DiffView component
 * into your application for comparing two query execution plans.
 */

import { DiffView } from './DiffView';
import type { ParsedPlan } from '../../types';

// Example: Comparing plans from two different optimization strategies

const plan1: ParsedPlan = {
  rootNodeId: 'node-1',
  nodes: [
    {
      id: 'node-1',
      operation: 'Hash Join',
      relation: null,
      cost: { startup: 10.0, total: 1000.0 },
      rows: 10000,
      children: ['node-2', 'node-3'],
      metadata: {},
    },
    {
      id: 'node-2',
      operation: 'Seq Scan',
      relation: 'users',
      cost: { startup: 0.0, total: 500.0 },
      rows: 5000,
      children: [],
      metadata: {},
    },
    {
      id: 'node-3',
      operation: 'Seq Scan',
      relation: 'orders',
      cost: { startup: 0.0, total: 450.0 },
      rows: 4500,
      children: [],
      metadata: {},
    },
  ],
  edges: [
    { from: 'node-1', to: 'node-2', rows: 5000 },
    { from: 'node-1', to: 'node-3', rows: 4500 },
  ],
};

const plan2: ParsedPlan = {
  rootNodeId: 'node-1',
  nodes: [
    {
      id: 'node-1',
      operation: 'Nested Loop',
      relation: null,
      cost: { startup: 5.0, total: 800.0 },
      rows: 8000,
      children: ['node-2', 'node-3'],
      metadata: {},
    },
    {
      id: 'node-2',
      operation: 'Index Scan',
      relation: 'users',
      cost: { startup: 0.0, total: 300.0 },
      rows: 3000,
      children: [],
      metadata: {},
    },
    {
      id: 'node-3',
      operation: 'Index Scan',
      relation: 'orders',
      cost: { startup: 0.0, total: 450.0 },
      rows: 4500,
      children: [],
      metadata: {},
    },
  ],
  edges: [
    { from: 'node-1', to: 'node-2', rows: 3000 },
    { from: 'node-1', to: 'node-3', rows: 4500 },
  ],
};

export function DiffViewExample() {
  const handleNodeClick = (nodeId: string, planIndex: 1 | 2) => {
    console.log(`Clicked node ${nodeId} in plan ${planIndex}`);
  };

  return (
    <DiffView
      plan1={plan1}
      plan2={plan2}
      onNodeClick={handleNodeClick}
    />
  );
}

/**
 * Integration with comparison mode:
 *
 * import { DiffView } from './components/comparison';
 *
 * function ComparisonView({ panels }) {
 *   if (panels.length !== 2) {
 *     return <Typography>Please select exactly 2 plans to compare</Typography>;
 *   }
 *
 *   const [panel1, panel2] = panels;
 *
 *   if (!panel1.parsedPlan || !panel2.parsedPlan) {
 *     return <Typography>Waiting for plans to load...</Typography>;
 *   }
 *
 *   return (
 *     <DiffView
 *       plan1={panel1.parsedPlan}
 *       plan2={panel2.parsedPlan}
 *       onNodeClick={(nodeId, planIndex) => {
 *         // Highlight the node in the corresponding panel
 *         const targetPanel = planIndex === 1 ? panel1 : panel2;
 *         highlightNodeInPanel(targetPanel.id, nodeId);
 *       }}
 *     />
 *   );
 * }
 */

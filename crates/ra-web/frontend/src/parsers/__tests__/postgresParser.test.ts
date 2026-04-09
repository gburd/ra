import { describe, it, expect, beforeEach } from 'vitest';
import { parsePostgresPlan } from '../postgresParser';
import type { ParsedPlan } from '../../types';

describe('postgresParser', () => {
  beforeEach(() => {
    (parsePostgresPlan as any).nodeIdCounter = 0;
  });

  describe('parsePostgresPlan', () => {
    it('parses simple sequential scan', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Seq Scan',
          'Relation Name': 'employees',
          'Startup Cost': 0.0,
          'Total Cost': 35.5,
          'Plan Rows': 2550,
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]).toMatchObject({
        operation: 'Seq Scan',
        relation: 'employees',
        cost: { startup: 0.0, total: 35.5 },
        rows: 2550,
      });
      expect(result!.edges).toHaveLength(0);
    });

    it('parses plan with actual execution times', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Index Scan',
          'Relation Name': 'users',
          'Startup Cost': 0.29,
          'Total Cost': 8.31,
          'Plan Rows': 1,
          'Actual Startup Time': 0.015,
          'Actual Total Time': 0.023,
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.actualTime).toEqual({
        startup: 0.015,
        total: 0.023,
      });
    });

    it('parses nested plan with multiple children', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Hash Join',
          'Startup Cost': 43.0,
          'Total Cost': 86.25,
          'Plan Rows': 1000,
          Plans: [
            {
              'Node Type': 'Seq Scan',
              'Relation Name': 'employees',
              'Startup Cost': 0.0,
              'Total Cost': 35.5,
              'Plan Rows': 2550,
            },
            {
              'Node Type': 'Hash',
              'Startup Cost': 22.0,
              'Total Cost': 22.0,
              'Plan Rows': 1200,
              Plans: [
                {
                  'Node Type': 'Seq Scan',
                  'Relation Name': 'departments',
                  'Startup Cost': 0.0,
                  'Total Cost': 22.0,
                  'Plan Rows': 1200,
                },
              ],
            },
          ],
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(4);
      expect(result!.edges).toHaveLength(3);

      const rootNode = result!.nodes.find(n => n.operation === 'Hash Join');
      expect(rootNode).toBeDefined();
      expect(rootNode!.children).toHaveLength(2);

      const hashNode = result!.nodes.find(n => n.operation === 'Hash');
      expect(hashNode).toBeDefined();
      expect(hashNode!.children).toHaveLength(1);
    });

    it('handles array wrapper format', () => {
      const rawPlan = JSON.stringify([
        {
          Plan: {
            'Node Type': 'Sort',
            'Startup Cost': 158.39,
            'Total Cost': 164.64,
            'Plan Rows': 2550,
          },
        },
      ]);

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]!.operation).toBe('Sort');
    });

    it('handles plan without wrapper', () => {
      const rawPlan = JSON.stringify({
        'Node Type': 'Aggregate',
        'Startup Cost': 48.32,
        'Total Cost': 48.33,
        'Plan Rows': 1,
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]!.operation).toBe('Aggregate');
    });

    it('parses complex multi-level plan', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Nested Loop',
          'Startup Cost': 8.59,
          'Total Cost': 105.45,
          'Plan Rows': 500,
          Plans: [
            {
              'Node Type': 'Hash Join',
              'Startup Cost': 8.31,
              'Total Cost': 70.20,
              'Plan Rows': 1000,
              Plans: [
                {
                  'Node Type': 'Seq Scan',
                  'Relation Name': 'orders',
                  'Startup Cost': 0.0,
                  'Total Cost': 32.0,
                  'Plan Rows': 2200,
                },
                {
                  'Node Type': 'Hash',
                  'Startup Cost': 8.0,
                  'Total Cost': 8.0,
                  'Plan Rows': 31,
                  Plans: [
                    {
                      'Node Type': 'Seq Scan',
                      'Relation Name': 'customers',
                      'Startup Cost': 0.0,
                      'Total Cost': 8.0,
                      'Plan Rows': 31,
                    },
                  ],
                },
              ],
            },
            {
              'Node Type': 'Index Scan',
              'Relation Name': 'products',
              'Startup Cost': 0.28,
              'Total Cost': 0.35,
              'Plan Rows': 1,
            },
          ],
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(6);
      expect(result!.edges).toHaveLength(5);

      expect(result!.nodes[0]!.operation).toBe('Nested Loop');
      expect(result!.nodes[0]!.children).toHaveLength(2);

      const joinNode = result!.nodes.find(n => n.operation === 'Hash Join');
      expect(joinNode).toBeDefined();
      expect(joinNode!.children).toHaveLength(2);
    });

    it('preserves metadata', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Bitmap Heap Scan',
          'Relation Name': 'users',
          'Startup Cost': 4.44,
          'Total Cost': 14.08,
          'Plan Rows': 10,
          'Recheck Cond': '(user_id < 100)',
          'Filter': '(active = true)',
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.metadata).toHaveProperty('Recheck Cond');
      expect(result!.nodes[0]!.metadata).toHaveProperty('Filter');
    });

    it('handles missing optional fields', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Result',
          'Startup Cost': 0.0,
          'Total Cost': 0.01,
          'Plan Rows': 1,
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.relation).toBeNull();
      expect(result!.nodes[0]!.actualTime).toBeUndefined();
    });
  });

  describe('error handling', () => {
    it('returns null for invalid JSON', () => {
      const rawPlan = 'not valid json {';

      const result = parsePostgresPlan(rawPlan);

      expect(result).toBeNull();
    });

    it('returns null for empty string', () => {
      const rawPlan = '';

      const result = parsePostgresPlan(rawPlan);

      expect(result).toBeNull();
    });

    it('handles malformed plan structure', () => {
      const rawPlan = JSON.stringify({
        invalid: 'structure',
        missing: 'required fields',
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
    });

    it('handles null input gracefully', () => {
      const result = parsePostgresPlan(null as any);

      expect(result).toBeNull();
    });

    it('handles undefined input gracefully', () => {
      const result = parsePostgresPlan(undefined as any);

      expect(result).toBeNull();
    });
  });

  describe('node extraction', () => {
    it('extracts all nodes from deep hierarchy', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Sort',
          'Startup Cost': 100.0,
          'Total Cost': 120.0,
          'Plan Rows': 1000,
          Plans: [
            {
              'Node Type': 'Merge Join',
              'Startup Cost': 80.0,
              'Total Cost': 100.0,
              'Plan Rows': 1000,
              Plans: [
                {
                  'Node Type': 'Sort',
                  'Startup Cost': 35.0,
                  'Total Cost': 40.0,
                  'Plan Rows': 500,
                  Plans: [
                    {
                      'Node Type': 'Seq Scan',
                      'Relation Name': 'table1',
                      'Startup Cost': 0.0,
                      'Total Cost': 35.0,
                      'Plan Rows': 500,
                    },
                  ],
                },
                {
                  'Node Type': 'Sort',
                  'Startup Cost': 42.0,
                  'Total Cost': 48.0,
                  'Plan Rows': 600,
                  Plans: [
                    {
                      'Node Type': 'Seq Scan',
                      'Relation Name': 'table2',
                      'Startup Cost': 0.0,
                      'Total Cost': 42.0,
                      'Plan Rows': 600,
                    },
                  ],
                },
              ],
            },
          ],
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(6);
      expect(result!.edges).toHaveLength(5);
    });

    it('correctly assigns parent-child relationships', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Hash Join',
          'Startup Cost': 10.0,
          'Total Cost': 50.0,
          'Plan Rows': 100,
          Plans: [
            {
              'Node Type': 'Seq Scan',
              'Relation Name': 'a',
              'Startup Cost': 0.0,
              'Total Cost': 20.0,
              'Plan Rows': 50,
            },
            {
              'Node Type': 'Hash',
              'Startup Cost': 5.0,
              'Total Cost': 15.0,
              'Plan Rows': 30,
              Plans: [
                {
                  'Node Type': 'Seq Scan',
                  'Relation Name': 'b',
                  'Startup Cost': 0.0,
                  'Total Cost': 10.0,
                  'Plan Rows': 30,
                },
              ],
            },
          ],
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();

      const rootNode = result!.nodes[0];
      expect(rootNode!.children).toHaveLength(2);

      const hashNode = result!.nodes[2];
      expect(hashNode!.children).toHaveLength(1);

      const edges = result!.edges;
      expect(edges).toHaveLength(3);
      expect(edges[0]!.from).toBe(rootNode!.id);
      expect(edges[1]!.from).toBe(rootNode!.id);
      expect(edges[2]!.from).toBe(hashNode!.id);
    });
  });

  describe('edge extraction', () => {
    it('creates edges with correct row estimates', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Nested Loop',
          'Startup Cost': 0.0,
          'Total Cost': 100.0,
          'Plan Rows': 1000,
          Plans: [
            {
              'Node Type': 'Seq Scan',
              'Relation Name': 'orders',
              'Startup Cost': 0.0,
              'Total Cost': 50.0,
              'Plan Rows': 500,
            },
            {
              'Node Type': 'Index Scan',
              'Relation Name': 'items',
              'Startup Cost': 0.0,
              'Total Cost': 10.0,
              'Plan Rows': 2,
            },
          ],
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(2);
      expect(result!.edges[0]!.rows).toBe(500);
      expect(result!.edges[1]!.rows).toBe(2);
    });

    it('creates no edges for single node plan', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Seq Scan',
          'Relation Name': 'test',
          'Startup Cost': 0.0,
          'Total Cost': 10.0,
          'Plan Rows': 100,
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(0);
    });
  });

  describe('cost extraction', () => {
    it('extracts startup and total costs', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Sort',
          'Startup Cost': 158.39,
          'Total Cost': 164.64,
          'Plan Rows': 2550,
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost).toEqual({
        startup: 158.39,
        total: 164.64,
      });
    });

    it('handles zero costs', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Result',
          'Startup Cost': 0.0,
          'Total Cost': 0.0,
          'Plan Rows': 1,
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost).toEqual({
        startup: 0.0,
        total: 0.0,
      });
    });

    it('handles very large costs', () => {
      const rawPlan = JSON.stringify({
        Plan: {
          'Node Type': 'Seq Scan',
          'Relation Name': 'huge_table',
          'Startup Cost': 0.0,
          'Total Cost': 1234567.89,
          'Plan Rows': 10000000,
        },
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost.total).toBe(1234567.89);
      expect(result!.nodes[0]!.rows).toBe(10000000);
    });
  });

  describe('realistic examples', () => {
    it('parses actual PostgreSQL EXPLAIN JSON output', () => {
      const rawPlan = JSON.stringify({
        'Plan': {
          'Node Type': 'Aggregate',
          'Strategy': 'Plain',
          'Partial Mode': 'Simple',
          'Startup Cost': 48.32,
          'Total Cost': 48.33,
          'Plan Rows': 1,
          'Plan Width': 8,
          'Actual Startup Time': 0.156,
          'Actual Total Time': 0.157,
          'Actual Rows': 1,
          'Actual Loops': 1,
          'Plans': [
            {
              'Node Type': 'Hash Join',
              'Parent Relationship': 'Outer',
              'Join Type': 'Inner',
              'Startup Cost': 8.31,
              'Total Cost': 48.29,
              'Plan Rows': 10,
              'Plan Width': 0,
              'Actual Startup Time': 0.065,
              'Actual Total Time': 0.145,
              'Actual Rows': 12,
              'Actual Loops': 1,
              'Hash Cond': '(orders.customer_id = customers.id)',
              'Plans': [
                {
                  'Node Type': 'Seq Scan',
                  'Parent Relationship': 'Outer',
                  'Relation Name': 'orders',
                  'Alias': 'orders',
                  'Startup Cost': 0.0,
                  'Total Cost': 32.0,
                  'Plan Rows': 2200,
                  'Plan Width': 4,
                  'Actual Startup Time': 0.010,
                  'Actual Total Time': 0.055,
                  'Actual Rows': 2200,
                  'Actual Loops': 1,
                },
                {
                  'Node Type': 'Hash',
                  'Parent Relationship': 'Inner',
                  'Startup Cost': 8.0,
                  'Total Cost': 8.0,
                  'Plan Rows': 31,
                  'Plan Width': 4,
                  'Actual Startup Time': 0.048,
                  'Actual Total Time': 0.048,
                  'Actual Rows': 31,
                  'Actual Loops': 1,
                  'Hash Buckets': 1024,
                  'Original Hash Buckets': 1024,
                  'Hash Batches': 1,
                  'Original Hash Batches': 1,
                  'Peak Memory Usage': 9,
                  'Plans': [
                    {
                      'Node Type': 'Seq Scan',
                      'Parent Relationship': 'Outer',
                      'Relation Name': 'customers',
                      'Alias': 'customers',
                      'Startup Cost': 0.0,
                      'Total Cost': 8.0,
                      'Plan Rows': 31,
                      'Plan Width': 4,
                      'Actual Startup Time': 0.004,
                      'Actual Total Time': 0.016,
                      'Actual Rows': 31,
                      'Actual Loops': 1,
                      'Filter': '(active = true)',
                      'Rows Removed by Filter': 169,
                    },
                  ],
                },
              ],
            },
          ],
        },
        'Planning Time': 0.123,
        'Execution Time': 0.234,
      });

      const result = parsePostgresPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(5);
      expect(result!.nodes[0]!.operation).toBe('Aggregate');
      expect(result!.nodes[1]!.operation).toBe('Hash Join');

      const scanNode = result!.nodes.find(
        n => n.operation === 'Seq Scan' && n.relation === 'customers'
      );
      expect(scanNode).toBeDefined();
      expect(scanNode!.metadata).toHaveProperty('Filter');
    });
  });
});

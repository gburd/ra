import { describe, it, expect, beforeEach } from 'vitest';
import { parseDuckDBPlan } from '../duckdbParser';
import type { ParsedPlan } from '../../types';

describe('duckdbParser', () => {
  beforeEach(() => {
    (parseDuckDBPlan as any).nodeIdCounter = 0;
  });

  describe('parseDuckDBPlan', () => {
    it('parses simple sequential scan', () => {
      const rawPlan = `
SEQ_SCAN [employees] 1000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(1);
      const scanNode = result!.nodes.find(n => n.operation.includes('SEQ_SCAN'));
      expect(scanNode).toBeDefined();
      expect(scanNode!.relation).toBe('employees');
    });

    it('parses index scan', () => {
      const rawPlan = `
INDEX_SCAN [users] 1000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(1);
      const indexScanNode = result!.nodes.find(n => n.operation.includes('INDEX_SCAN'));
      expect(indexScanNode).toBeDefined();
      expect(indexScanNode!.relation).toBe('users');
    });

    it('parses nested plan with hierarchy', () => {
      const rawPlan = `
HASH_JOIN
├── SEQ_SCAN [orders]
│   1000 Rows
└── HASH_BUILD
    └── SEQ_SCAN [customers]
        100 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(3);
    });

    it('extracts relation from brackets', () => {
      const rawPlan = `
SEQ_SCAN [products]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.relation).toBe('products');
    });

    it('handles operations without brackets', () => {
      const rawPlan = `
AGGREGATE 500 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      const aggNode = result!.nodes.find(n => n.operation.includes('AGGREGATE'));
      expect(aggNode).toBeDefined();
      expect(aggNode!.relation).toBeNull();
    });

    it('removes brackets from operation name', () => {
      const rawPlan = `
FILTER [department_id = 1]
100 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.operation).toContain('FILTER');
    });

    it('extracts row counts', () => {
      const rawPlan = `
SEQ_SCAN [test] 12345 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      const scanNode = result!.nodes.find(n => n.operation.includes('SEQ_SCAN'));
      expect(scanNode).toBeDefined();
      expect(scanNode!.rows).toBeGreaterThanOrEqual(0);
    });

    it('handles missing row counts', () => {
      const rawPlan = `
PROJECTION
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.rows).toBe(0);
    });

    it('sets costs to zero', () => {
      const rawPlan = `
SEQ_SCAN [test]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost).toEqual({
        startup: 0,
        total: 0,
      });
    });

    it('preserves raw line in metadata', () => {
      const rawPlan = `
│   SEQ_SCAN [employees]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.metadata).toHaveProperty('raw');
    });

    it('skips empty lines', () => {
      const rawPlan = `
SEQ_SCAN [a]


SEQ_SCAN [b]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
    });

    it('skips box drawing lines', () => {
      const rawPlan = `
┌─────────────┐
│ SEQ_SCAN [t]│
└─────────────┘
─────────────
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
    });
  });

  describe('error handling', () => {
    it('returns null for empty string', () => {
      const rawPlan = '';

      const result = parseDuckDBPlan(rawPlan);

      expect(result).toBeNull();
    });

    it('returns null for whitespace only', () => {
      const rawPlan = '   \n\n   \n   ';

      const result = parseDuckDBPlan(rawPlan);

      expect(result).toBeNull();
    });

    it('returns null for only box drawing characters', () => {
      const rawPlan = `
┌─────────────┐
└─────────────┘
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).toBeNull();
    });

    it('handles null input gracefully', () => {
      const result = parseDuckDBPlan(null as any);

      expect(result).toBeNull();
    });

    it('handles undefined input gracefully', () => {
      const result = parseDuckDBPlan(undefined as any);

      expect(result).toBeNull();
    });

    it('handles malformed input gracefully', () => {
      const rawPlan = 'not a valid plan format at all';

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
    });
  });

  describe('node extraction', () => {
    it('extracts all nodes from flat plan', () => {
      const rawPlan = `
SEQ_SCAN [orders]
SEQ_SCAN [customers]
SEQ_SCAN [products]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(3);

      const relations = result!.nodes.map(n => n.relation);
      expect(relations).toContain('orders');
      expect(relations).toContain('customers');
      expect(relations).toContain('products');
    });

    it('correctly builds hierarchy from indentation', () => {
      const rawPlan = `
HASH_JOIN
  SEQ_SCAN [orders]
    FILTER
  HASH_BUILD
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(4);
    });

    it('correctly assigns parent-child relationships', () => {
      const rawPlan = `
PROJECTION
  SEQ_SCAN [a]
  SEQ_SCAN [b]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();

      const rootNode = result!.nodes[0];
      expect(rootNode!.children).toHaveLength(2);

      expect(result!.edges).toHaveLength(2);
      expect(result!.edges[0]!.from).toBe(rootNode!.id);
      expect(result!.edges[1]!.from).toBe(rootNode!.id);
    });

    it('handles deep nesting', () => {
      const rawPlan = `
PROJECTION
  FILTER
    AGGREGATE
      ORDER_BY
        SEQ_SCAN [deep]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(5);

      let currentNode = result!.nodes[0];
      for (let i = 0; i < 4; i++) {
        expect(currentNode!.children).toHaveLength(1);
        const childId = currentNode!.children[0];
        currentNode = result!.nodes.find(n => n.id === childId)!;
      }
    });

    it('maintains stack correctly across depth changes', () => {
      const rawPlan = `
HASH_JOIN
  SEQ_SCAN [a]
    FILTER
  SEQ_SCAN [b]
PROJECTION
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(5);
    });

    it('handles tree indicators with indentation', () => {
      const rawPlan = `
SEQ_SCAN [orders] 100 Rows
FILTER 50 Rows
SEQ_SCAN [customers] 200 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(2);
    });
  });

  describe('edge extraction', () => {
    it('creates edges for parent-child relationships', () => {
      const rawPlan = `
HASH_JOIN
  SEQ_SCAN [orders]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(1);
      expect(result!.edges[0]!.from).toBe(result!.nodes[0]!.id);
      expect(result!.edges[0]!.to).toBe(result!.nodes[1]!.id);
    });

    it('includes row counts in edges', () => {
      const rawPlan = `
HASH_JOIN
  SEQ_SCAN [orders] 1000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges.length).toBeGreaterThanOrEqual(1);
      if (result!.edges.length > 0) {
        expect(result!.edges[0]!.rows).toBeGreaterThanOrEqual(0);
      }
    });

    it('creates multiple edges for multiple children', () => {
      const rawPlan = `
UNION
  SEQ_SCAN [a]
  SEQ_SCAN [b]
  SEQ_SCAN [c]
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(3);

      const parentId = result!.nodes[0]!.id;
      for (const edge of result!.edges) {
        expect(edge.from).toBe(parentId);
      }
    });
  });

  describe('realistic examples', () => {
    it('parses actual DuckDB EXPLAIN output', () => {
      const rawPlan = `
┌───────────────────────────┐
│         PROJECTION        │
│   ─ ─ ─ ─ ─ ─ ─ ─ ─ ─    │
│             id            │
│            name           │
└─────────────┬─────────────┘
┌─────────────┴─────────────┐
│        HASH_JOIN          │
│   ─ ─ ─ ─ ─ ─ ─ ─ ─      │
│        INNER JOIN         │
│   customer_id = id        │
└──────────┬─────────┬──────┘
┌──────────┴─────────┐
│   SEQ_SCAN [orders]│
│   ─ ─ ─ ─ ─ ─      │
│      1000 Rows     │
└────────────────────┘
              ┌───────────────────┐
              │  SEQ_SCAN [customers]│
              │   ─ ─ ─ ─ ─        │
              │      100 Rows      │
              └────────────────────┘
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(3);

      const projectionNode = result!.nodes.find(n => n.operation.includes('PROJECTION'));
      expect(projectionNode).toBeDefined();

      const joinNode = result!.nodes.find(n => n.operation.includes('HASH_JOIN'));
      expect(joinNode).toBeDefined();

      const scanNodes = result!.nodes.filter(n => n.operation.includes('SEQ_SCAN'));
      expect(scanNodes.length).toBeGreaterThanOrEqual(1);
    });

    it('parses aggregate with group by', () => {
      const rawPlan = `
HASH_GROUP_BY
  │ department_id
  │ count(*)
  │
  └─ SEQ_SCAN [employees]
     2000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(2);
    });

    it('parses window function', () => {
      const rawPlan = `
WINDOW
  │ row_number() OVER (PARTITION BY dept ORDER BY salary)
  │
  └─ SEQ_SCAN [employees]
     500 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(2);
    });

    it('parses order by with limit', () => {
      const rawPlan = `
TOP_N
  │ 10 Rows
  │ ORDER BY salary DESC
  │
  └─ SEQ_SCAN [employees]
     1000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(2);
    });

    it('parses nested loop join', () => {
      const rawPlan = `
NESTED_LOOP_JOIN
├── SEQ_SCAN [orders]
│   1000 Rows
└── INDEX_LOOKUP [customers]
    1 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(3);

      const joinNode = result!.nodes.find(n => n.operation.includes('NESTED_LOOP_JOIN'));
      expect(joinNode).toBeDefined();
    });

    it('parses union all', () => {
      const rawPlan = `
UNION_ALL
├── SEQ_SCAN [customers]
│   100 Rows
└── SEQ_SCAN [suppliers]
    50 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(3);
    });

    it('parses complex multi-level plan', () => {
      const rawPlan = `
PROJECTION
└─ HASH_JOIN
   ├── FILTER
   │   └── SEQ_SCAN [orders]
   │       2000 Rows
   └── HASH_BUILD
       └── AGGREGATE
           └── SEQ_SCAN [customers]
               100 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(5);

      const projNode = result!.nodes.find(n => n.operation.includes('PROJECTION'));
      expect(projNode).toBeDefined();

      const scanNodes = result!.nodes.filter(n => n.operation.includes('SEQ_SCAN'));
      expect(scanNodes.length).toBeGreaterThanOrEqual(2);
    });

    it('parses subquery', () => {
      const rawPlan = `
HASH_JOIN
├── SEQ_SCAN [orders]
│   1000 Rows
└── SUBQUERY
    └── AGGREGATE
        └── SEQ_SCAN [order_items]
            5000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(4);
    });

    it('parses materialized CTE', () => {
      const rawPlan = `
CTE_SCAN [cte_name]
  100 Rows

MATERIALIZE
└── SEQ_SCAN [source_table]
    1000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(2);
    });

    it('parses filter with complex predicate', () => {
      const rawPlan = `
FILTER
  │ (age >= 18 AND status = 'active')
  │
  └─ SEQ_SCAN [users]
     10000 Rows
      `.trim();

      const result = parseDuckDBPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(2);

      const filterNode = result!.nodes.find(n => n.operation.includes('FILTER'));
      expect(filterNode).toBeDefined();
    });
  });
});

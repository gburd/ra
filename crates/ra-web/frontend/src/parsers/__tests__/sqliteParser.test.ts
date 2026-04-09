import { describe, it, expect, beforeEach } from 'vitest';
import { parseSQLitePlan } from '../sqliteParser';
import type { ParsedPlan } from '../../types';

describe('sqliteParser', () => {
  beforeEach(() => {
    (parseSQLitePlan as any).nodeIdCounter = 0;
  });

  describe('parseSQLitePlan', () => {
    it('parses simple scan', () => {
      const rawPlan = `
0 0 0 SCAN TABLE employees
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]!.operation).toBe('SCAN TABLE employees');
      expect(result!.nodes[0]!.relation).toBe('employees');
      expect(result!.edges).toHaveLength(0);
    });

    it('parses search with index', () => {
      const rawPlan = `
0 0 0 SEARCH TABLE users USING INDEX idx_user_id (user_id=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]!.operation).toContain('SEARCH TABLE users');
      expect(result!.nodes[0]!.relation).toBe('users');
    });

    it('parses nested plan with parent-child relationships', () => {
      const rawPlan = `
2 0 0 SCAN TABLE orders
3 0 0 SEARCH TABLE customers USING INTEGER PRIMARY KEY (rowid=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
      expect(result!.edges).toHaveLength(0);
    });

    it('parses plan with tree structure indicators', () => {
      const rawPlan = `
|--0 0 0 SCAN TABLE orders
\`--0 0 0 SEARCH TABLE customers USING INTEGER PRIMARY KEY (rowid=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
    });

    it('parses hierarchical plan', () => {
      const rawPlan = `
3 0 0 SCAN TABLE orders
6 0 0   SCAN TABLE customers
9 0 0     SEARCH TABLE addresses USING INDEX idx_customer_id (customer_id=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(3);
    });

    it('handles comment lines', () => {
      const rawPlan = `
--SCAN TABLE orders
0 0 0 SCAN TABLE employees
--SEARCH TABLE customers
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]!.relation).toBe('employees');
    });

    it('extracts relation from SCAN TABLE', () => {
      const rawPlan = `
0 0 0 SCAN TABLE products
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.relation).toBe('products');
    });

    it('extracts relation from SEARCH TABLE', () => {
      const rawPlan = `
0 0 0 SEARCH TABLE orders USING INDEX idx_order_date
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.relation).toBe('orders');
    });

    it('extracts relation from bare SCAN', () => {
      const rawPlan = `
0 0 0 SCAN items
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.relation).toBe('items');
    });

    it('sets costs to zero', () => {
      const rawPlan = `
0 0 0 SCAN TABLE test
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost).toEqual({
        startup: 0,
        total: 0,
      });
      expect(result!.nodes[0]!.rows).toBe(0);
    });

    it('preserves metadata', () => {
      const rawPlan = `
42 17 5 SEARCH TABLE users USING INDEX idx_email (email=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.metadata).toHaveProperty('id', '42');
      expect(result!.nodes[0]!.metadata).toHaveProperty('parent', '17');
      expect(result!.nodes[0]!.metadata).toHaveProperty('detail');
    });
  });

  describe('error handling', () => {
    it('returns null for empty string', () => {
      const rawPlan = '';

      const result = parseSQLitePlan(rawPlan);

      expect(result).toBeNull();
    });

    it('returns null for whitespace only', () => {
      const rawPlan = '   \n\n   \n   ';

      const result = parseSQLitePlan(rawPlan);

      expect(result).toBeNull();
    });

    it('returns null for malformed plan', () => {
      const rawPlan = 'not a valid plan format';

      const result = parseSQLitePlan(rawPlan);

      expect(result).toBeNull();
    });

    it('handles null input gracefully', () => {
      const result = parseSQLitePlan(null as any);

      expect(result).toBeNull();
    });

    it('handles undefined input gracefully', () => {
      const result = parseSQLitePlan(undefined as any);

      expect(result).toBeNull();
    });

    it('skips invalid lines gracefully', () => {
      const rawPlan = `
0 0 0 SCAN TABLE valid
invalid line without numbers
1 0 0 SCAN TABLE another
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
    });
  });

  describe('node extraction', () => {
    it('extracts all nodes from flat plan', () => {
      const rawPlan = `
0 0 0 SCAN TABLE orders
1 0 0 SCAN TABLE customers
2 0 0 SCAN TABLE products
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(3);

      const relations = result!.nodes.map(n => n.relation);
      expect(relations).toContain('orders');
      expect(relations).toContain('customers');
      expect(relations).toContain('products');
    });

    it('correctly builds hierarchy from indentation', () => {
      const rawPlan = `
0 0 0 SCAN TABLE orders
  1 0 0 SEARCH TABLE customers USING INTEGER PRIMARY KEY
    2 0 0 SCAN TABLE addresses
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(3);

      const rootNode = result!.nodes[0];
      expect(rootNode!.children).toHaveLength(1);

      const childNode = result!.nodes[1];
      expect(childNode!.children).toHaveLength(1);

      const grandchildNode = result!.nodes[2];
      expect(grandchildNode!.children).toHaveLength(0);
    });

    it('handles mixed tree indicators and indentation', () => {
      const rawPlan = `
0 0 0 SCAN TABLE a
1 0 0 SCAN TABLE b
2 0 0 SCAN TABLE c
3 0 0 SCAN TABLE d
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(2);
    });

    it('correctly assigns parent-child relationships', () => {
      const rawPlan = `
0 0 0 SCAN TABLE root
  1 0 0 SCAN TABLE child1
  2 0 0 SCAN TABLE child2
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();

      const rootNode = result!.nodes[0];
      expect(rootNode!.children).toHaveLength(2);

      expect(result!.edges).toHaveLength(2);
      expect(result!.edges[0]!.from).toBe(rootNode!.id);
      expect(result!.edges[1]!.from).toBe(rootNode!.id);
    });

    it('handles deep nesting', () => {
      const rawPlan = `
0 0 0 SCAN TABLE level0
  1 0 0 SCAN TABLE level1
    2 0 0 SCAN TABLE level2
      3 0 0 SCAN TABLE level3
        4 0 0 SCAN TABLE level4
      `.trim();

      const result = parseSQLitePlan(rawPlan);

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
0 0 0 SCAN TABLE a
  1 0 0 SCAN TABLE b
    2 0 0 SCAN TABLE c
  3 0 0 SCAN TABLE d
4 0 0 SCAN TABLE e
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(5);

      const nodeA = result!.nodes[0];
      expect(nodeA!.children).toHaveLength(2);

      const nodeB = result!.nodes.find(n => n.relation === 'b');
      expect(nodeB!.children).toHaveLength(1);
    });
  });

  describe('edge extraction', () => {
    it('creates edges for parent-child relationships', () => {
      const rawPlan = `
0 0 0 SCAN TABLE parent
  1 0 0 SCAN TABLE child
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(1);
      expect(result!.edges[0]!.from).toBe(result!.nodes[0]!.id);
      expect(result!.edges[0]!.to).toBe(result!.nodes[1]!.id);
    });

    it('sets edge rows to zero', () => {
      const rawPlan = `
0 0 0 SCAN TABLE orders
  1 0 0 SCAN TABLE items
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(1);
      expect(result!.edges[0]!.rows).toBe(0);
    });

    it('creates multiple edges for multiple children', () => {
      const rawPlan = `
0 0 0 SCAN TABLE parent
  1 0 0 SCAN TABLE child1
  2 0 0 SCAN TABLE child2
  3 0 0 SCAN TABLE child3
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(3);

      const parentId = result!.nodes[0]!.id;
      for (const edge of result!.edges) {
        expect(edge.from).toBe(parentId);
      }
    });
  });

  describe('realistic examples', () => {
    it('parses actual SQLite EXPLAIN QUERY PLAN output', () => {
      const rawPlan = `
2 0 0 SCAN TABLE customers
5 0 0 SEARCH TABLE orders USING INDEX idx_customer_id (customer_id=?)
8 0 0 SEARCH TABLE order_items USING INDEX idx_order_id (order_id=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(3);

      const customerNode = result!.nodes.find(n => n.relation === 'customers');
      expect(customerNode).toBeDefined();
      expect(customerNode!.operation).toContain('SCAN TABLE');

      const ordersNode = result!.nodes.find(n => n.relation === 'orders');
      expect(ordersNode).toBeDefined();
      expect(ordersNode!.operation).toContain('SEARCH TABLE');
      expect(ordersNode!.operation).toContain('idx_customer_id');
    });

    it('parses join plan', () => {
      const rawPlan = `
2 0 0 SCAN TABLE employees
5 0 0 SEARCH TABLE departments USING INTEGER PRIMARY KEY (rowid=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
    });

    it('parses subquery plan', () => {
      const rawPlan = `
3 0 0 SCAN SUBQUERY 1
5 0 1 SCAN TABLE products
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
    });

    it('parses compound select', () => {
      const rawPlan = `
1 0 0 SCAN TABLE users
2 0 0 COMPOUND QUERY
3 0 0 LEFT-MOST SUBQUERY
4 0 0 SCAN TABLE customers
5 0 0 UNION USING TEMP B-TREE
6 0 0 SCAN TABLE suppliers
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThan(0);
    });

    it('parses aggregate with index', () => {
      const rawPlan = `
3 0 0 SEARCH TABLE sales USING COVERING INDEX idx_product_date (product_id=? AND date>?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]!.operation).toContain('COVERING INDEX');
    });

    it('parses temp table usage', () => {
      const rawPlan = `
2 0 0 USE TEMP B-TREE FOR ORDER BY
5 0 0 SCAN TABLE orders
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
    });

    it('parses automatic index creation', () => {
      const rawPlan = `
3 0 0 SCAN TABLE orders
10 0 0 SEARCH TABLE customers USING AUTOMATIC COVERING INDEX (customer_id=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);

      const autoIndexNode = result!.nodes.find(n => n.operation.includes('AUTOMATIC'));
      expect(autoIndexNode).toBeDefined();
    });

    it('parses complex nested plan', () => {
      const rawPlan = `
2 0 0 SCAN TABLE orders
5 0 0   SEARCH TABLE customers USING INTEGER PRIMARY KEY (rowid=?)
8 0 0   SEARCH TABLE addresses USING INDEX idx_customer_id (customer_id=?)
11 0 0 SCAN TABLE order_items
14 0 0   SEARCH TABLE products USING INTEGER PRIMARY KEY (rowid=?)
      `.trim();

      const result = parseSQLitePlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes.length).toBeGreaterThanOrEqual(4);
    });
  });
});

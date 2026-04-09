import { describe, it, expect, beforeEach } from 'vitest';
import { parseMySQLPlan } from '../mysqlParser';
import type { ParsedPlan } from '../../types';

describe('mysqlParser', () => {
  beforeEach(() => {
    (parseMySQLPlan as any).nodeIdCounter = 0;
  });

  describe('parseMySQLPlan', () => {
    it('parses simple table scan', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          cost_info: {
            query_cost: '1005.00',
          },
          table: {
            table_name: 'employees',
            access_type: 'ALL',
            rows_examined_per_scan: 1000,
            cost_info: {
              read_cost: '800.00',
              eval_cost: '100.00',
              prefix_cost: '1005.00',
              data_read_per_join: '100K',
            },
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);

      const rootNode = result!.nodes[0];
      expect(rootNode!.operation).toBe('Query Block');
      expect(rootNode!.cost.total).toBe(1005.0);

      const scanNode = result!.nodes[1];
      expect(scanNode!.operation).toBe('Seq Scan');
      expect(scanNode!.relation).toBe('employees');
      expect(scanNode!.rows).toBe(1000);
      expect(scanNode!.cost.total).toBe(900.0);
    });

    it('distinguishes between full scan and index scan', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          table: {
            table_name: 'users',
            access_type: 'ref',
            key: 'idx_user_id',
            rows_examined_per_scan: 1,
            cost_info: {
              read_cost: '1.00',
              eval_cost: '0.10',
              prefix_cost: '1.10',
              data_read_per_join: '256',
            },
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      const scanNode = result!.nodes.find(n => n.relation === 'users');
      expect(scanNode).toBeDefined();
      expect(scanNode!.operation).toBe('Index Scan');
      expect(scanNode!.metadata).toHaveProperty('key');
    });

    it('parses multiple tables', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          cost_info: {
            query_cost: '2050.50',
          },
          table: [
            {
              table_name: 'orders',
              access_type: 'ALL',
              rows_examined_per_scan: 1000,
              cost_info: {
                read_cost: '800.00',
                eval_cost: '100.00',
                prefix_cost: '1005.00',
                data_read_per_join: '50K',
              },
            },
            {
              table_name: 'customers',
              access_type: 'eq_ref',
              key: 'PRIMARY',
              rows_examined_per_scan: 1,
              cost_info: {
                read_cost: '1000.00',
                eval_cost: '200.00',
                prefix_cost: '2005.00',
                data_read_per_join: '30K',
              },
            },
          ],
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(3);

      const ordersScan = result!.nodes.find(n => n.relation === 'orders');
      expect(ordersScan).toBeDefined();
      expect(ordersScan!.operation).toBe('Seq Scan');

      const customersScan = result!.nodes.find(n => n.relation === 'customers');
      expect(customersScan).toBeDefined();
      expect(customersScan!.operation).toBe('Index Scan');
    });

    it('parses nested loop joins', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          cost_info: {
            query_cost: '3010.25',
          },
          nested_loop: [
            {
              table: {
                table_name: 'orders',
                access_type: 'ALL',
                rows_examined_per_scan: 1500,
                cost_info: {
                  read_cost: '1200.00',
                  eval_cost: '150.00',
                  prefix_cost: '1350.00',
                  data_read_per_join: '75K',
                },
              },
            },
            {
              table: {
                table_name: 'order_items',
                access_type: 'ref',
                key: 'idx_order_id',
                rows_examined_per_scan: 2,
                cost_info: {
                  read_cost: '1500.00',
                  eval_cost: '300.00',
                  prefix_cost: '3150.00',
                  data_read_per_join: '60K',
                },
              },
            },
          ],
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(3);

      const rootNode = result!.nodes[0];
      expect(rootNode!.children).toHaveLength(2);

      const ordersScan = result!.nodes.find(n => n.relation === 'orders');
      expect(ordersScan).toBeDefined();

      const itemsScan = result!.nodes.find(n => n.relation === 'order_items');
      expect(itemsScan).toBeDefined();
    });

    it('handles plan without query_block wrapper', () => {
      const rawPlan = JSON.stringify({
        select_id: 1,
        cost_info: {
          query_cost: '500.00',
        },
        table: {
          table_name: 'test',
          access_type: 'ALL',
          rows_examined_per_scan: 500,
          cost_info: {
            read_cost: '400.00',
            eval_cost: '50.00',
            prefix_cost: '500.00',
            data_read_per_join: '25K',
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(2);
      expect(result!.nodes[0]!.operation).toBe('Query Block');
    });

    it('handles missing cost_info', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          table: {
            table_name: 'simple',
            access_type: 'ALL',
            rows_examined_per_scan: 100,
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost.total).toBe(0);
      expect(result!.nodes[1]!.cost.total).toBe(0);
    });

    it('preserves metadata', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          table: {
            table_name: 'users',
            access_type: 'range',
            possible_keys: ['idx_age', 'idx_status'],
            key: 'idx_age',
            key_length: '4',
            used_key_parts: ['age'],
            rows_examined_per_scan: 50,
            filtered: '100.00',
            cost_info: {
              read_cost: '40.00',
              eval_cost: '5.00',
              prefix_cost: '50.00',
              data_read_per_join: '10K',
            },
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      const scanNode = result!.nodes.find(n => n.relation === 'users');
      expect(scanNode!.metadata).toHaveProperty('possible_keys');
      expect(scanNode!.metadata).toHaveProperty('key');
      expect(scanNode!.metadata).toHaveProperty('key_length');
    });
  });

  describe('error handling', () => {
    it('returns null for invalid JSON', () => {
      const rawPlan = 'invalid json {';

      const result = parseMySQLPlan(rawPlan);

      expect(result).toBeNull();
    });

    it('returns null for empty string', () => {
      const rawPlan = '';

      const result = parseMySQLPlan(rawPlan);

      expect(result).toBeNull();
    });

    it('handles null input gracefully', () => {
      const result = parseMySQLPlan(null as any);

      expect(result).toBeNull();
    });

    it('handles undefined input gracefully', () => {
      const result = parseMySQLPlan(undefined as any);

      expect(result).toBeNull();
    });

    it('returns plan even without tables', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          cost_info: {
            query_cost: '0.00',
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(1);
      expect(result!.nodes[0]!.children).toHaveLength(0);
    });
  });

  describe('node extraction', () => {
    it('extracts all table nodes', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          table: [
            {
              table_name: 't1',
              access_type: 'ALL',
              rows_examined_per_scan: 100,
              cost_info: {
                read_cost: '80.00',
                eval_cost: '10.00',
                prefix_cost: '100.00',
                data_read_per_join: '5K',
              },
            },
            {
              table_name: 't2',
              access_type: 'ref',
              key: 'idx_1',
              rows_examined_per_scan: 10,
              cost_info: {
                read_cost: '90.00',
                eval_cost: '1.00',
                prefix_cost: '101.00',
                data_read_per_join: '2K',
              },
            },
            {
              table_name: 't3',
              access_type: 'eq_ref',
              key: 'PRIMARY',
              rows_examined_per_scan: 1,
              cost_info: {
                read_cost: '95.00',
                eval_cost: '0.10',
                prefix_cost: '101.10',
                data_read_per_join: '1K',
              },
            },
          ],
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(4);

      const tableNodes = result!.nodes.slice(1);
      expect(tableNodes[0]!.relation).toBe('t1');
      expect(tableNodes[1]!.relation).toBe('t2');
      expect(tableNodes[2]!.relation).toBe('t3');
    });

    it('correctly assigns parent-child relationships', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          nested_loop: [
            {
              table: {
                table_name: 'parent',
                access_type: 'ALL',
                rows_examined_per_scan: 100,
                cost_info: {
                  read_cost: '50.00',
                  eval_cost: '10.00',
                  prefix_cost: '100.00',
                  data_read_per_join: '10K',
                },
              },
            },
            {
              table: {
                table_name: 'child',
                access_type: 'ref',
                rows_examined_per_scan: 5,
                cost_info: {
                  read_cost: '400.00',
                  eval_cost: '50.00',
                  prefix_cost: '550.00',
                  data_read_per_join: '20K',
                },
              },
            },
          ],
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();

      const rootNode = result!.nodes[0];
      expect(rootNode!.children).toHaveLength(2);

      expect(result!.edges).toHaveLength(2);
      expect(result!.edges[0]!.from).toBe(rootNode!.id);
      expect(result!.edges[1]!.from).toBe(rootNode!.id);
    });
  });

  describe('edge extraction', () => {
    it('creates edges with correct row estimates', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          nested_loop: [
            {
              table: {
                table_name: 'orders',
                access_type: 'ALL',
                rows_examined_per_scan: 1000,
                cost_info: {
                  read_cost: '800.00',
                  eval_cost: '100.00',
                  prefix_cost: '1000.00',
                  data_read_per_join: '50K',
                },
              },
            },
            {
              table: {
                table_name: 'items',
                access_type: 'ref',
                rows_examined_per_scan: 3,
                cost_info: {
                  read_cost: '2400.00',
                  eval_cost: '300.00',
                  prefix_cost: '3700.00',
                  data_read_per_join: '30K',
                },
              },
            },
          ],
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(2);
      expect(result!.edges[0]!.rows).toBe(1000);
      expect(result!.edges[1]!.rows).toBe(3);
    });

    it('creates edges for table array format', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          table: [
            {
              table_name: 'a',
              access_type: 'ALL',
              rows_examined_per_scan: 50,
              cost_info: {
                read_cost: '40.00',
                eval_cost: '5.00',
                prefix_cost: '50.00',
                data_read_per_join: '5K',
              },
            },
            {
              table_name: 'b',
              access_type: 'ALL',
              rows_examined_per_scan: 30,
              cost_info: {
                read_cost: '24.00',
                eval_cost: '3.00',
                prefix_cost: '77.00',
                data_read_per_join: '3K',
              },
            },
          ],
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.edges).toHaveLength(2);
    });
  });

  describe('cost extraction', () => {
    it('calculates cost from read_cost and eval_cost', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          cost_info: {
            query_cost: '1234.56',
          },
          table: {
            table_name: 'test',
            access_type: 'ALL',
            rows_examined_per_scan: 1000,
            cost_info: {
              read_cost: '1000.00',
              eval_cost: '100.00',
              prefix_cost: '1234.56',
              data_read_per_join: '100K',
            },
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost.total).toBe(1234.56);
      expect(result!.nodes[1]!.cost.total).toBe(1100.0);
    });

    it('handles zero costs', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          cost_info: {
            query_cost: '0.00',
          },
          table: {
            table_name: 'empty',
            access_type: 'const',
            rows_examined_per_scan: 0,
            cost_info: {
              read_cost: '0.00',
              eval_cost: '0.00',
              prefix_cost: '0.00',
              data_read_per_join: '0',
            },
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost.total).toBe(0.0);
      expect(result!.nodes[1]!.cost.total).toBe(0.0);
    });

    it('sets startup cost to zero', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          table: {
            table_name: 'test',
            access_type: 'ALL',
            rows_examined_per_scan: 100,
            cost_info: {
              read_cost: '80.00',
              eval_cost: '10.00',
              prefix_cost: '100.00',
              data_read_per_join: '10K',
            },
          },
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes[0]!.cost.startup).toBe(0);
      expect(result!.nodes[1]!.cost.startup).toBe(0);
    });
  });

  describe('realistic examples', () => {
    it('parses actual MySQL EXPLAIN FORMAT=JSON output', () => {
      const rawPlan = JSON.stringify({
        query_block: {
          select_id: 1,
          cost_info: {
            query_cost: '3158.76',
          },
          nested_loop: [
            {
              table: {
                table_name: 'customers',
                access_type: 'ALL',
                possible_keys: ['PRIMARY'],
                rows_examined_per_scan: 200,
                rows_produced_per_join: 200,
                filtered: '100.00',
                cost_info: {
                  read_cost: '20.50',
                  eval_cost: '20.00',
                  prefix_cost: '40.50',
                  data_read_per_join: '62K',
                },
                used_columns: [
                  'id',
                  'name',
                  'email',
                  'created_at',
                ],
              },
            },
            {
              table: {
                table_name: 'orders',
                access_type: 'ref',
                possible_keys: ['idx_customer_id', 'idx_order_date'],
                key: 'idx_customer_id',
                used_key_parts: ['customer_id'],
                key_length: '4',
                ref: ['test.customers.id'],
                rows_examined_per_scan: 15,
                rows_produced_per_join: 3000,
                filtered: '100.00',
                using_index: false,
                cost_info: {
                  read_cost: '3000.00',
                  eval_cost: '300.00',
                  prefix_cost: '3340.50',
                  data_read_per_join: '468K',
                },
                used_columns: [
                  'id',
                  'customer_id',
                  'order_date',
                  'total',
                ],
              },
            },
          ],
        },
      });

      const result = parseMySQLPlan(rawPlan);

      expect(result).not.toBeNull();
      expect(result!.nodes).toHaveLength(3);
      expect(result!.nodes[0]!.operation).toBe('Query Block');

      const customerScan = result!.nodes.find(n => n.relation === 'customers');
      expect(customerScan).toBeDefined();
      expect(customerScan!.operation).toBe('Seq Scan');
      expect(customerScan!.rows).toBe(200);

      const orderScan = result!.nodes.find(n => n.relation === 'orders');
      expect(orderScan).toBeDefined();
      expect(orderScan!.operation).toBe('Index Scan');
      expect(orderScan!.rows).toBe(15);
      expect(orderScan!.metadata).toHaveProperty('key', 'idx_customer_id');
    });
  });
});

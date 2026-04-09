import { describe, it, expect } from 'vitest';
import {
  parseCost,
  parseActual,
  parseTimingLine,
  formatTime,
  formatNumber,
  getIndentLevel,
  extractOperation,
  findMatches,
} from '../planParser';

describe('planParser', () => {
  describe('parseCost', () => {
    it('parses cost estimate with rows and width', () => {
      const line = 'Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32)';
      const cost = parseCost(line);

      expect(cost).toEqual({
        startup: 0.0,
        total: 35.5,
        rows: 2550,
        width: 32,
      });
    });

    it('returns null for lines without cost', () => {
      const line = 'Filter: (department_id = 1)';
      const cost = parseCost(line);

      expect(cost).toBeNull();
    });
  });

  describe('parseActual', () => {
    it('parses actual stats from EXPLAIN ANALYZE', () => {
      const line = 'Seq Scan on employees  (actual time=0.012..0.234 rows=1000 loops=1)';
      const actual = parseActual(line);

      expect(actual).toEqual({
        time: 0.234,
        rows: 1000,
        loops: 1,
      });
    });

    it('returns null for lines without actual stats', () => {
      const line = 'Seq Scan on employees  (cost=0.00..35.50 rows=2550 width=32)';
      const actual = parseActual(line);

      expect(actual).toBeNull();
    });
  });

  describe('parseTimingLine', () => {
    it('parses Planning Time', () => {
      const line = 'Planning Time: 0.123 ms';
      const timing = parseTimingLine(line);

      expect(timing).toEqual({
        label: 'Planning',
        value: 0.123,
      });
    });

    it('parses Execution Time', () => {
      const line = 'Execution Time: 0.456 ms';
      const timing = parseTimingLine(line);

      expect(timing).toEqual({
        label: 'Execution',
        value: 0.456,
      });
    });
  });

  describe('formatTime', () => {
    it('formats microseconds', () => {
      expect(formatTime(0.123)).toBe('123µs');
    });

    it('formats milliseconds', () => {
      expect(formatTime(12.345)).toBe('12.35ms');
    });

    it('formats seconds', () => {
      expect(formatTime(1234.5)).toBe('1.23s');
    });
  });

  describe('formatNumber', () => {
    it('formats numbers with thousands separators', () => {
      expect(formatNumber(1234567)).toBe('1,234,567');
      expect(formatNumber(123)).toBe('123');
    });
  });

  describe('getIndentLevel', () => {
    it('calculates indent level from leading spaces', () => {
      expect(getIndentLevel('Seq Scan')).toBe(0);
      expect(getIndentLevel('  Filter')).toBe(1);
      expect(getIndentLevel('    Rows Removed')).toBe(2);
    });
  });

  describe('extractOperation', () => {
    it('extracts common operation names', () => {
      expect(extractOperation('Seq Scan on employees')).toBe('Seq Scan');
      expect(extractOperation('Hash Join')).toBe('Hash Join');
      expect(extractOperation('Sort')).toBe('Sort');
      expect(extractOperation('Filter: (dept = 1)')).toBe('Filter');
    });

    it('returns null for non-operation lines', () => {
      expect(extractOperation('Planning Time: 0.123 ms')).toBeNull();
    });
  });

  describe('findMatches', () => {
    it('finds all matches for a search term', () => {
      const planText = `Seq Scan on employees
  Filter: department_id = 1
  Rows Removed by Filter: 500`;

      const matches = findMatches(planText, 'Filter');

      expect(matches).toHaveLength(2);
      expect(matches[0]).toEqual({ lineIndex: 1, charIndex: 2 });
      expect(matches[1]).toEqual({ lineIndex: 2, charIndex: 18 });
    });

    it('returns empty array for no matches', () => {
      const planText = 'Seq Scan on employees';
      const matches = findMatches(planText, 'NonExistent');

      expect(matches).toEqual([]);
    });

    it('returns empty array for empty search term', () => {
      const planText = 'Seq Scan on employees';
      const matches = findMatches(planText, '');

      expect(matches).toEqual([]);
    });
  });
});

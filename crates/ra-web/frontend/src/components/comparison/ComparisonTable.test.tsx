import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ComparisonTable } from './ComparisonTable';
import type { CostMetrics } from '../../types';

describe('ComparisonTable', () => {
  const mockMetrics: CostMetrics[] = [
    {
      totalCost: 100.5,
      totalRows: 1000,
      planDepth: 3,
      operationBreakdown: [
        {
          nodeId: '1',
          operation: 'Seq Scan on users',
          cost: 50.25,
          rows: 500,
          percentage: 50.0,
        },
        {
          nodeId: '2',
          operation: 'Index Scan on orders',
          cost: 30.15,
          rows: 300,
          percentage: 30.0,
        },
        {
          nodeId: '3',
          operation: 'Hash Join',
          cost: 20.1,
          rows: 200,
          percentage: 20.0,
        },
      ],
    },
    {
      totalCost: 85.3,
      totalRows: 950,
      planDepth: 2,
      operationBreakdown: [
        {
          nodeId: '1',
          operation: 'Index Scan on users',
          cost: 45.2,
          rows: 450,
          percentage: 53.0,
        },
        {
          nodeId: '2',
          operation: 'Nested Loop',
          cost: 40.1,
          rows: 500,
          percentage: 47.0,
        },
      ],
    },
  ];

  const engineNames = ['PostgreSQL 15', 'MySQL 8.4'];

  it('renders table with engine columns', () => {
    render(<ComparisonTable metrics={mockMetrics} engineNames={engineNames} />);

    expect(screen.getByText('PostgreSQL 15')).toBeInTheDocument();
    expect(screen.getByText('MySQL 8.4')).toBeInTheDocument();
  });

  it('displays all metric rows', () => {
    render(<ComparisonTable metrics={mockMetrics} engineNames={engineNames} />);

    expect(screen.getByText('Total Cost')).toBeInTheDocument();
    expect(screen.getByText('Estimated Rows')).toBeInTheDocument();
    expect(screen.getByText('Plan Depth')).toBeInTheDocument();
    expect(screen.getByText('Scan Operations')).toBeInTheDocument();
    expect(screen.getByText('Join Operations')).toBeInTheDocument();
    expect(screen.getByText('Sort Operations')).toBeInTheDocument();
    expect(screen.getByText('Index Usage')).toBeInTheDocument();
  });

  it('formats cost values correctly', () => {
    render(<ComparisonTable metrics={mockMetrics} engineNames={engineNames} />);

    expect(screen.getByText('100.50')).toBeInTheDocument();
    expect(screen.getByText('85.30')).toBeInTheDocument();
  });

  it('displays index usage correctly', () => {
    render(<ComparisonTable metrics={mockMetrics} engineNames={engineNames} />);

    const yesElements = screen.getAllByText('Yes');
    expect(yesElements.length).toBe(2);
  });

  it('counts operations correctly', () => {
    render(<ComparisonTable metrics={mockMetrics} engineNames={engineNames} />);

    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('1')).toBeInTheDocument();
  });

  it('displays warning note about cost comparison', () => {
    render(<ComparisonTable metrics={mockMetrics} engineNames={engineNames} />);

    expect(
      screen.getByText(/Cost values are engine-specific and not directly comparable/i)
    ).toBeInTheDocument();
  });

  it('handles single metric', () => {
    render(<ComparisonTable metrics={[mockMetrics[0]]} engineNames={['PostgreSQL 15']} />);

    expect(screen.getByText('PostgreSQL 15')).toBeInTheDocument();
    expect(screen.getByText('100.50')).toBeInTheDocument();
  });

  it('handles four engines', () => {
    const fourMetrics = [
      mockMetrics[0],
      mockMetrics[1],
      {
        totalCost: 120.0,
        totalRows: 1100,
        planDepth: 4,
        operationBreakdown: [
          {
            nodeId: '1',
            operation: 'Table Scan',
            cost: 60.0,
            rows: 600,
            percentage: 50.0,
          },
          {
            nodeId: '2',
            operation: 'Sort',
            cost: 60.0,
            rows: 500,
            percentage: 50.0,
          },
        ],
      },
      {
        totalCost: 95.5,
        totalRows: 1050,
        planDepth: 3,
        operationBreakdown: [
          {
            nodeId: '1',
            operation: 'Index Only Scan',
            cost: 55.0,
            rows: 550,
            percentage: 57.6,
          },
          {
            nodeId: '2',
            operation: 'Merge Join',
            cost: 40.5,
            rows: 500,
            percentage: 42.4,
          },
        ],
      },
    ];

    const fourEngines = ['PostgreSQL 15', 'MySQL 8.4', 'MariaDB 11', 'DuckDB'];

    render(<ComparisonTable metrics={fourMetrics} engineNames={fourEngines} />);

    expect(screen.getByText('PostgreSQL 15')).toBeInTheDocument();
    expect(screen.getByText('MySQL 8.4')).toBeInTheDocument();
    expect(screen.getByText('MariaDB 11')).toBeInTheDocument();
    expect(screen.getByText('DuckDB')).toBeInTheDocument();
  });
});

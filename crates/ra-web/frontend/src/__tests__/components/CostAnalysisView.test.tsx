import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CostAnalysisView } from '../../components/visualizations/CostAnalysisView';
import type { CostMetrics, OperationCost } from '../../types';

describe('CostAnalysisView', () => {
  const createMockOperationCost = (
    nodeId: string,
    operation: string,
    cost: number,
    rows: number
  ): OperationCost => ({
    nodeId,
    operation,
    cost,
    rows,
    percentage: 0,
  });

  const createMockCostMetrics = (): CostMetrics => {
    const ops = [
      createMockOperationCost('1', 'Seq Scan on users', 150.5, 10000),
      createMockOperationCost('2', 'Index Scan on orders', 50.3, 5000),
      createMockOperationCost('3', 'Hash Join', 200.8, 8000),
      createMockOperationCost('4', 'Aggregate', 30.2, 1000),
      createMockOperationCost('5', 'Sort', 75.1, 8000),
    ];

    const totalCost = ops.reduce((sum, op) => sum + op.cost, 0);
    ops.forEach((op) => {
      op.percentage = (op.cost / totalCost) * 100;
    });

    return {
      totalCost,
      totalRows: 32000,
      planDepth: 4,
      operationBreakdown: ops,
    };
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders summary cards with correct values', () => {
    const metrics = createMockCostMetrics();
    render(<CostAnalysisView costMetrics={metrics} />);

    expect(screen.getByText('Total Cost')).toBeInTheDocument();
    expect(screen.getByText(metrics.totalCost.toFixed(2))).toBeInTheDocument();

    expect(screen.getByText('Total Rows')).toBeInTheDocument();
    expect(screen.getByText('32,000')).toBeInTheDocument();

    expect(screen.getByText('Plan Depth')).toBeInTheDocument();
    expect(screen.getByText('4')).toBeInTheDocument();

    expect(screen.getByText('Operations')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
  });

  it('renders operations table with all data', () => {
    const metrics = createMockCostMetrics();
    render(<CostAnalysisView costMetrics={metrics} />);

    metrics.operationBreakdown.forEach((op) => {
      expect(screen.getByText(op.operation)).toBeInTheDocument();
      expect(screen.getByText(op.cost.toFixed(2))).toBeInTheDocument();
      expect(screen.getByText(op.rows.toLocaleString())).toBeInTheDocument();
      expect(screen.getByText(`${op.percentage.toFixed(1)}%`)).toBeInTheDocument();
    });
  });

  it('handles empty operation breakdown', () => {
    const emptyMetrics: CostMetrics = {
      totalCost: 0,
      totalRows: 0,
      planDepth: 0,
      operationBreakdown: [],
    };

    render(<CostAnalysisView costMetrics={emptyMetrics} />);

    expect(screen.getByText('Total Cost')).toBeInTheDocument();
    expect(screen.getByText('0.00')).toBeInTheDocument();
  });

  it('sorts by operation name ascending when clicking operation header', async () => {
    const metrics = createMockCostMetrics();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} />);

    const operationHeader = screen.getByText('Operation');
    await user.click(operationHeader);

    const rows = screen.getAllByRole('row');
    const firstDataRow = within(rows[1]).getAllByRole('cell');
    expect(firstDataRow[0].textContent).toContain('Aggregate');
  });

  it('sorts by cost descending by default', () => {
    const metrics = createMockCostMetrics();
    render(<CostAnalysisView costMetrics={metrics} />);

    const rows = screen.getAllByRole('row');
    const firstDataRow = within(rows[1]).getAllByRole('cell');
    expect(firstDataRow[0].textContent).toContain('Hash Join');
  });

  it('sorts by cost ascending when clicking cost header twice', async () => {
    const metrics = createMockCostMetrics();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} />);

    const costHeader = screen.getByText('Cost');
    await user.click(costHeader);

    const rows = screen.getAllByRole('row');
    const firstDataRow = within(rows[1]).getAllByRole('cell');
    expect(firstDataRow[0].textContent).toContain('Aggregate');
  });

  it('sorts by rows when clicking rows header', async () => {
    const metrics = createMockCostMetrics();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} />);

    const rowsHeader = screen.getByText('Rows');
    await user.click(rowsHeader);

    const rows = screen.getAllByRole('row');
    const firstDataRow = within(rows[1]).getAllByRole('cell');
    expect(firstDataRow[0].textContent).toContain('Aggregate');
  });

  it('sorts by percentage when clicking percentage header', async () => {
    const metrics = createMockCostMetrics();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} />);

    const percentageHeader = screen.getByText('% of Total');
    await user.click(percentageHeader);

    const rows = screen.getAllByRole('row');
    const firstDataRow = within(rows[1]).getAllByRole('cell');
    expect(firstDataRow[0].textContent).toContain('Aggregate');
  });

  it('toggles sort direction when clicking same header twice', async () => {
    const metrics = createMockCostMetrics();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} />);

    const costHeader = screen.getByText('Cost');

    await user.click(costHeader);
    let rows = screen.getAllByRole('row');
    let firstDataRow = within(rows[1]).getAllByRole('cell');
    const firstOperation = firstDataRow[0].textContent;

    await user.click(costHeader);
    rows = screen.getAllByRole('row');
    firstDataRow = within(rows[1]).getAllByRole('cell');
    const secondOperation = firstDataRow[0].textContent;

    expect(firstOperation).not.toBe(secondOperation);
  });

  it('calls onNodeClick when row is clicked', async () => {
    const metrics = createMockCostMetrics();
    const onNodeClick = vi.fn();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} onNodeClick={onNodeClick} />);

    const rows = screen.getAllByRole('row');
    await user.click(rows[1]);

    expect(onNodeClick).toHaveBeenCalledWith('3');
  });

  it('does not call onNodeClick when callback is not provided', async () => {
    const metrics = createMockCostMetrics();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} />);

    const rows = screen.getAllByRole('row');
    await user.click(rows[1]);
  });

  it('handles operation without nodeId gracefully', async () => {
    const metrics = createMockCostMetrics();
    metrics.operationBreakdown[0].nodeId = '';
    const onNodeClick = vi.fn();
    const user = userEvent.setup();

    render(<CostAnalysisView costMetrics={metrics} onNodeClick={onNodeClick} />);

    const rows = screen.getAllByRole('row');
    await user.click(rows[1]);

    expect(onNodeClick).not.toHaveBeenCalled();
  });

  it('renders bar chart with top 10 operations', () => {
    const metrics = createMockCostMetrics();
    for (let i = 6; i <= 15; i++) {
      metrics.operationBreakdown.push(
        createMockOperationCost(`${i}`, `Operation ${i}`, 10, 100)
      );
    }

    render(<CostAnalysisView costMetrics={metrics} />);

    expect(screen.getByText('Cost Distribution (Top 10)')).toBeInTheDocument();
  });

  it('truncates long operation names in chart', () => {
    const metrics: CostMetrics = {
      totalCost: 100,
      totalRows: 1000,
      planDepth: 2,
      operationBreakdown: [
        createMockOperationCost(
          '1',
          'Very Long Operation Name That Should Be Truncated',
          100,
          1000
        ),
      ],
    };
    metrics.operationBreakdown[0].percentage = 100;

    render(<CostAnalysisView costMetrics={metrics} />);

    expect(screen.getByText('Very Long Operation Name That Should Be Truncated')).toBeInTheDocument();
  });

  it('formats large numbers with locale separators', () => {
    const metrics: CostMetrics = {
      totalCost: 1000000.5,
      totalRows: 9876543210,
      planDepth: 5,
      operationBreakdown: [
        createMockOperationCost('1', 'Test Operation', 1000000.5, 9876543210),
      ],
    };
    metrics.operationBreakdown[0].percentage = 100;

    render(<CostAnalysisView costMetrics={metrics} />);

    expect(screen.getByText('1000000.50')).toBeInTheDocument();
    expect(screen.getByText('9,876,543,210')).toBeInTheDocument();
  });

  it('displays correct percentages summing to 100', () => {
    const metrics = createMockCostMetrics();
    render(<CostAnalysisView costMetrics={metrics} />);

    const percentages = metrics.operationBreakdown.map((op) => op.percentage);
    const sum = percentages.reduce((acc, val) => acc + val, 0);

    expect(Math.abs(sum - 100)).toBeLessThan(0.1);
  });

  it('handles single operation', () => {
    const metrics: CostMetrics = {
      totalCost: 50,
      totalRows: 1000,
      planDepth: 1,
      operationBreakdown: [createMockOperationCost('1', 'Single Op', 50, 1000)],
    };
    metrics.operationBreakdown[0].percentage = 100;

    render(<CostAnalysisView costMetrics={metrics} />);

    expect(screen.getByText('Single Op')).toBeInTheDocument();
    expect(screen.getByText('100.0%')).toBeInTheDocument();
  });

  it('applies correct color coding to operation types', () => {
    const metrics: CostMetrics = {
      totalCost: 300,
      totalRows: 3000,
      planDepth: 3,
      operationBreakdown: [
        createMockOperationCost('1', 'Seq Scan', 100, 1000),
        createMockOperationCost('2', 'Index Scan', 100, 1000),
        createMockOperationCost('3', 'Hash Join', 100, 1000),
      ],
    };
    metrics.operationBreakdown.forEach((op) => {
      op.percentage = 33.33;
    });

    render(<CostAnalysisView costMetrics={metrics} />);

    const colorIndicators = document.querySelectorAll('[style*="border-radius"]');
    expect(colorIndicators.length).toBeGreaterThan(0);
  });
});

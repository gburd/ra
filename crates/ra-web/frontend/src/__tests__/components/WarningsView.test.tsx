import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { WarningsView } from '../../components/visualizations/WarningsView';
import type { Warning } from '../../types';

describe('WarningsView', () => {
  const createMockWarning = (
    severity: Warning['severity'],
    type: Warning['type'],
    message: string,
    nodeId: string,
    suggestion: string
  ): Warning => ({
    severity,
    type,
    message,
    nodeId,
    suggestion,
  });

  const createMockWarnings = (): Warning[] => [
    createMockWarning(
      'critical',
      'full_table_scan',
      'Full table scan detected on large table',
      '1',
      'Consider adding an index on the filter column'
    ),
    createMockWarning(
      'warning',
      'expensive_sort',
      'Large sort operation detected',
      '2',
      'Add an index to avoid sorting or reduce result set'
    ),
    createMockWarning(
      'info',
      'missing_statistics',
      'Table statistics may be outdated',
      '3',
      'Run ANALYZE on the table to update statistics'
    ),
    createMockWarning(
      'critical',
      'cartesian_product',
      'Cartesian product detected in join',
      '4',
      'Add join conditions to prevent cross product'
    ),
  ];

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders success message when no warnings', () => {
    render(<WarningsView warnings={[]} />);

    expect(screen.getByText('No Issues Detected')).toBeInTheDocument();
    expect(screen.getByText('The query plan looks good with no optimization warnings.')).toBeInTheDocument();
  });

  it('renders all warnings', () => {
    const warnings = createMockWarnings();
    render(<WarningsView warnings={warnings} />);

    warnings.forEach((warning) => {
      expect(screen.getByText(warning.message)).toBeInTheDocument();
      expect(screen.getByText(warning.suggestion)).toBeInTheDocument();
    });
  });

  it('displays correct severity counts', () => {
    const warnings = createMockWarnings();
    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('2 Critical')).toBeInTheDocument();
    expect(screen.getByText('1 Warnings')).toBeInTheDocument();
    expect(screen.getByText('1 Info')).toBeInTheDocument();
  });

  it('groups warnings by type', () => {
    const warnings = createMockWarnings();
    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('Full Table Scan')).toBeInTheDocument();
    expect(screen.getByText('Expensive Sort')).toBeInTheDocument();
    expect(screen.getByText('Missing Statistics')).toBeInTheDocument();
    expect(screen.getByText('Cartesian Product')).toBeInTheDocument();
  });

  it('shows issue count per warning type', () => {
    const warnings: Warning[] = [
      createMockWarning('warning', 'full_table_scan', 'Scan 1', '1', 'Fix 1'),
      createMockWarning('warning', 'full_table_scan', 'Scan 2', '2', 'Fix 2'),
      createMockWarning('info', 'expensive_sort', 'Sort', '3', 'Fix sort'),
    ];

    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('2 issues')).toBeInTheDocument();
    expect(screen.getByText('1 issue')).toBeInTheDocument();
  });

  it('expands accordion when clicked', async () => {
    const warnings = createMockWarnings();
    const user = userEvent.setup();

    render(<WarningsView warnings={warnings} />);

    const accordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    expect(accordion).toBeInTheDocument();

    await user.click(accordion!);

    const suggestion = await screen.findByText('Consider adding an index on the filter column');
    expect(suggestion).toBeInTheDocument();
  });

  it('collapses accordion when clicked twice', async () => {
    const warnings = createMockWarnings();
    const user = userEvent.setup();

    render(<WarningsView warnings={warnings} />);

    const accordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    await user.click(accordion!);

    let suggestion = await screen.findByText('Consider adding an index on the filter column');
    expect(suggestion).toBeInTheDocument();

    await user.click(accordion!);
  });

  it('calls onNodeClick when warning is clicked', async () => {
    const warnings = createMockWarnings();
    const onNodeClick = vi.fn();
    const user = userEvent.setup();

    render(<WarningsView warnings={warnings} onNodeClick={onNodeClick} />);

    const accordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    await user.click(accordion!);

    const alert = await screen.findByText('Full table scan detected on large table');
    const alertBox = alert.closest('[role="alert"]');

    await user.click(alertBox!);

    expect(onNodeClick).toHaveBeenCalledWith('1');
  });

  it('does not call onNodeClick when callback is not provided', async () => {
    const warnings = createMockWarnings();
    const user = userEvent.setup();

    render(<WarningsView warnings={warnings} />);

    const accordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    await user.click(accordion!);

    const alert = await screen.findByText('Full table scan detected on large table');
    const alertBox = alert.closest('[role="alert"]');
    await user.click(alertBox!);
  });

  it('displays node IDs in warnings', async () => {
    const warnings = createMockWarnings();
    const user = userEvent.setup();

    render(<WarningsView warnings={warnings} />);

    const accordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    await user.click(accordion!);

    expect(await screen.findByText(/Node ID: 1/)).toBeInTheDocument();
  });

  it('applies correct severity colors', () => {
    const warnings: Warning[] = [
      createMockWarning('critical', 'full_table_scan', 'Critical issue', '1', 'Fix it'),
      createMockWarning('warning', 'expensive_sort', 'Warning issue', '2', 'Fix it'),
      createMockWarning('info', 'missing_statistics', 'Info issue', '3', 'Fix it'),
    ];

    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('Full Table Scan')).toBeInTheDocument();
    expect(screen.getByText('Expensive Sort')).toBeInTheDocument();
    expect(screen.getByText('Missing Statistics')).toBeInTheDocument();
  });

  it('shows only critical severity count when others are zero', () => {
    const warnings: Warning[] = [
      createMockWarning('critical', 'full_table_scan', 'Issue', '1', 'Fix'),
    ];

    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('1 Critical')).toBeInTheDocument();
    expect(screen.queryByText(/Warnings/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Info/)).not.toBeInTheDocument();
  });

  it('shows only warning severity count when others are zero', () => {
    const warnings: Warning[] = [
      createMockWarning('warning', 'expensive_sort', 'Issue', '1', 'Fix'),
    ];

    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('1 Warnings')).toBeInTheDocument();
    expect(screen.queryByText(/Critical/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Info/)).not.toBeInTheDocument();
  });

  it('shows only info severity count when others are zero', () => {
    const warnings: Warning[] = [
      createMockWarning('info', 'missing_statistics', 'Issue', '1', 'Fix'),
    ];

    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('1 Info')).toBeInTheDocument();
    expect(screen.queryByText(/Critical/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Warnings/)).not.toBeInTheDocument();
  });

  it('handles multiple warnings of same type', async () => {
    const warnings: Warning[] = [
      createMockWarning('warning', 'full_table_scan', 'Scan on users', '1', 'Add index on user_id'),
      createMockWarning('warning', 'full_table_scan', 'Scan on orders', '2', 'Add index on order_id'),
      createMockWarning('warning', 'full_table_scan', 'Scan on products', '3', 'Add index on product_id'),
    ];

    const user = userEvent.setup();
    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('3 issues')).toBeInTheDocument();

    const accordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    await user.click(accordion!);

    expect(await screen.findByText('Scan on users')).toBeInTheDocument();
    expect(await screen.findByText('Scan on orders')).toBeInTheDocument();
    expect(await screen.findByText('Scan on products')).toBeInTheDocument();
  });

  it('displays all warning type labels correctly', () => {
    const warnings: Warning[] = [
      createMockWarning('info', 'full_table_scan', 'msg', '1', 'sug'),
      createMockWarning('info', 'cartesian_product', 'msg', '2', 'sug'),
      createMockWarning('info', 'missing_index', 'msg', '3', 'sug'),
      createMockWarning('info', 'expensive_sort', 'msg', '4', 'sug'),
      createMockWarning('info', 'inefficient_join', 'msg', '5', 'sug'),
      createMockWarning('info', 'missing_statistics', 'msg', '6', 'sug'),
    ];

    render(<WarningsView warnings={warnings} />);

    expect(screen.getByText('Full Table Scan')).toBeInTheDocument();
    expect(screen.getByText('Cartesian Product')).toBeInTheDocument();
    expect(screen.getByText('Missing Index')).toBeInTheDocument();
    expect(screen.getByText('Expensive Sort')).toBeInTheDocument();
    expect(screen.getByText('Inefficient Join')).toBeInTheDocument();
    expect(screen.getByText('Missing Statistics')).toBeInTheDocument();
  });

  it('maintains separate expansion state for each accordion', async () => {
    const warnings: Warning[] = [
      createMockWarning('warning', 'full_table_scan', 'Scan issue', '1', 'Fix scan'),
      createMockWarning('info', 'expensive_sort', 'Sort issue', '2', 'Fix sort'),
    ];

    const user = userEvent.setup();
    render(<WarningsView warnings={warnings} />);

    const scanAccordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    await user.click(scanAccordion!);

    expect(await screen.findByText('Fix scan')).toBeInTheDocument();

    const sortAccordion = screen.getByText('Expensive Sort').closest('div[role="button"]');
    await user.click(sortAccordion!);

    expect(await screen.findByText('Fix sort')).toBeInTheDocument();
  });

  it('renders with empty warnings array without crashing', () => {
    render(<WarningsView warnings={[]} />);
    expect(screen.getByText('No Issues Detected')).toBeInTheDocument();
  });

  it('handles warnings with special characters in messages', async () => {
    const warnings: Warning[] = [
      createMockWarning(
        'warning',
        'full_table_scan',
        'Table "users" has <special> & characters',
        '1',
        'Use `proper` escaping'
      ),
    ];

    const user = userEvent.setup();
    render(<WarningsView warnings={warnings} />);

    const accordion = screen.getByText('Full Table Scan').closest('div[role="button"]');
    await user.click(accordion!);

    expect(await screen.findByText('Table "users" has <special> & characters')).toBeInTheDocument();
  });
});

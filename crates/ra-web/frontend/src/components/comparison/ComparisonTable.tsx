import { useMemo } from 'react';
import {
  Box,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
  Chip,
} from '@mui/material';
import CheckCircleIcon from '@mui/icons-material/CheckCircle';
import CancelIcon from '@mui/icons-material/Cancel';
import type { CostMetrics } from '../../types';

interface ComparisonTableProps {
  metrics: CostMetrics[];
  engineNames: string[];
}

interface MetricRow {
  name: string;
  values: (number | string | boolean)[];
  unit?: string;
  isBestLowest?: boolean;
  format?: (value: number | string | boolean) => string;
}

function formatNumber(value: number): string {
  if (value >= 1000000) {
    return `${(value / 1000000).toFixed(2)}M`;
  }
  if (value >= 1000) {
    return `${(value / 1000).toFixed(2)}K`;
  }
  return value.toLocaleString();
}

function countOperationType(metrics: CostMetrics, type: string): number {
  return metrics.operationBreakdown.filter((op) =>
    op.operation.toLowerCase().includes(type.toLowerCase())
  ).length;
}

function hasIndexUsage(metrics: CostMetrics): boolean {
  return metrics.operationBreakdown.some((op) =>
    op.operation.toLowerCase().includes('index')
  );
}

export function ComparisonTable({ metrics, engineNames }: ComparisonTableProps) {
  const metricRows = useMemo<MetricRow[]>(() => {
    const rows: MetricRow[] = [
      {
        name: 'Total Cost',
        values: metrics.map((m) => m.totalCost),
        isBestLowest: true,
        format: (value) => (typeof value === 'number' ? value.toFixed(2) : String(value)),
      },
      {
        name: 'Estimated Rows',
        values: metrics.map((m) => m.totalRows),
        isBestLowest: true,
        format: (value) => (typeof value === 'number' ? formatNumber(value) : String(value)),
      },
      {
        name: 'Plan Depth',
        values: metrics.map((m) => m.planDepth),
        isBestLowest: true,
      },
      {
        name: 'Scan Operations',
        values: metrics.map((m) => countOperationType(m, 'scan')),
        isBestLowest: true,
      },
      {
        name: 'Join Operations',
        values: metrics.map((m) => countOperationType(m, 'join')),
        isBestLowest: true,
      },
      {
        name: 'Sort Operations',
        values: metrics.map((m) => countOperationType(m, 'sort')),
        isBestLowest: true,
      },
      {
        name: 'Index Usage',
        values: metrics.map((m) => hasIndexUsage(m)),
        format: (value) => (value ? 'Yes' : 'No'),
      },
    ];
    return rows;
  }, [metrics]);

  const getBestWorstIndices = (row: MetricRow): { best: number; worst: number } | null => {
    if (row.values.length <= 1) {
      return null;
    }

    const numericValues = row.values
      .map((v, idx) => ({ value: typeof v === 'number' ? v : null, idx }))
      .filter((item) => item.value !== null) as { value: number; idx: number }[];

    if (numericValues.length === 0) {
      return null;
    }

    if (row.isBestLowest) {
      const minValue = Math.min(...numericValues.map((item) => item.value));
      const maxValue = Math.max(...numericValues.map((item) => item.value));
      const bestIdx = numericValues.find((item) => item.value === minValue)!.idx;
      const worstIdx = numericValues.find((item) => item.value === maxValue)!.idx;
      return { best: bestIdx, worst: worstIdx };
    }

    return null;
  };

  const getPercentageOfMax = (value: number, row: MetricRow): number => {
    const numericValues = row.values.filter((v) => typeof v === 'number') as number[];
    if (numericValues.length === 0) {
      return 0;
    }
    const maxValue = Math.max(...numericValues);
    if (maxValue === 0) {
      return 0;
    }
    return (value / maxValue) * 100;
  };

  const getCellColor = (
    colIdx: number,
    indices: { best: number; worst: number } | null
  ): string => {
    if (!indices) {
      return '#1E293B';
    }
    if (colIdx === indices.best) {
      return '#064E3B';
    }
    if (colIdx === indices.worst) {
      return '#7F1D1D';
    }
    return '#1E293B';
  };

  return (
    <Box sx={{ width: '100%', height: '100%', overflow: 'auto', p: 2, bgcolor: '#0F172A' }}>
      <Box sx={{ mb: 2 }}>
        <Typography variant="h6" color="#F1F5F9" gutterBottom>
          Statistical Comparison
        </Typography>
        <Typography variant="body2" color="#94A3B8">
          Note: Cost values are engine-specific and not directly comparable across different
          database systems. Lower costs generally indicate better performance within the same
          engine.
        </Typography>
      </Box>

      <TableContainer component={Paper} sx={{ bgcolor: '#1E293B' }}>
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell sx={{ color: '#F1F5F9', fontWeight: 'bold', minWidth: 150 }}>
                Metric
              </TableCell>
              {engineNames.map((name, idx) => (
                <TableCell
                  key={idx}
                  align="center"
                  sx={{ color: '#F1F5F9', fontWeight: 'bold', minWidth: 120 }}
                >
                  {name}
                </TableCell>
              ))}
            </TableRow>
          </TableHead>
          <TableBody>
            {metricRows.map((row, rowIdx) => {
              const indices = getBestWorstIndices(row);
              return (
                <TableRow key={rowIdx} hover sx={{ '&:hover': { bgcolor: '#334155' } }}>
                  <TableCell sx={{ color: '#F1F5F9', fontWeight: 500 }}>{row.name}</TableCell>
                  {row.values.map((value, colIdx) => {
                    const formattedValue = row.format ? row.format(value) : String(value);
                    const percentage =
                      typeof value === 'number' && row.name === 'Total Cost'
                        ? getPercentageOfMax(value, row)
                        : null;

                    return (
                      <TableCell
                        key={colIdx}
                        align="center"
                        sx={{
                          color: '#F1F5F9',
                          bgcolor: getCellColor(colIdx, indices),
                          transition: 'background-color 0.2s',
                        }}
                      >
                        <Box
                          sx={{
                            display: 'flex',
                            flexDirection: 'column',
                            alignItems: 'center',
                            gap: 0.5,
                          }}
                        >
                          {row.name === 'Index Usage' ? (
                            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                              {value ? (
                                <>
                                  <CheckCircleIcon sx={{ color: '#34D399', fontSize: 18 }} />
                                  <span>{formattedValue}</span>
                                </>
                              ) : (
                                <>
                                  <CancelIcon sx={{ color: '#F87171', fontSize: 18 }} />
                                  <span>{formattedValue}</span>
                                </>
                              )}
                            </Box>
                          ) : (
                            <span>{formattedValue}</span>
                          )}
                          {percentage !== null && (
                            <Chip
                              label={`${percentage.toFixed(0)}%`}
                              size="small"
                              sx={{
                                height: 18,
                                fontSize: '0.7rem',
                                bgcolor:
                                  percentage < 50
                                    ? '#065F46'
                                    : percentage < 80
                                      ? '#92400E'
                                      : '#7F1D1D',
                                color: '#F1F5F9',
                              }}
                            />
                          )}
                        </Box>
                      </TableCell>
                    );
                  })}
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </TableContainer>
    </Box>
  );
}

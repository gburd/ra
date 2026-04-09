import { useState, useMemo } from 'react';
import {
  Box,
  Card,
  CardContent,
  Typography,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  TableSortLabel,
  Paper,
  Grid,
} from '@mui/material';
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  Cell,
} from 'recharts';
import type { CostMetrics } from '../../types';

interface CostAnalysisViewProps {
  costMetrics: CostMetrics;
  onNodeClick?: (nodeId: string) => void;
}

type SortField = 'operation' | 'cost' | 'rows' | 'percentage';
type SortDirection = 'asc' | 'desc';

const OPERATION_COLORS: Record<string, string> = {
  Scan: '#60A5FA',
  Index: '#34D399',
  Join: '#F87171',
  Aggregate: '#C084FC',
  Sort: '#FB923C',
  Filter: '#FCD34D',
};

function getOperationColor(operation: string): string {
  for (const [key, color] of Object.entries(OPERATION_COLORS)) {
    if (operation.includes(key)) {
      return color;
    }
  }
  return '#94A3B8';
}

export function CostAnalysisView({ costMetrics, onNodeClick }: CostAnalysisViewProps) {
  const [sortField, setSortField] = useState<SortField>('cost');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');

  const sortedOperations = useMemo(() => {
    const sorted = [...costMetrics.operationBreakdown];
    sorted.sort((a, b) => {
      let aVal: number | string = a[sortField];
      let bVal: number | string = b[sortField];

      if (typeof aVal === 'string') {
        return sortDirection === 'asc'
          ? aVal.localeCompare(bVal as string)
          : (bVal as string).localeCompare(aVal);
      }

      return sortDirection === 'asc' ? aVal - (bVal as number) : (bVal as number) - aVal;
    });
    return sorted;
  }, [costMetrics.operationBreakdown, sortField, sortDirection]);

  const handleSort = (field: SortField) => {
    if (sortField === field) {
      setSortDirection(sortDirection === 'asc' ? 'desc' : 'asc');
    } else {
      setSortField(field);
      setSortDirection('desc');
    }
  };

  const chartData = useMemo(() => {
    return costMetrics.operationBreakdown
      .slice(0, 10)
      .map((op) => ({
        operation: op.operation.length > 20 ? op.operation.substring(0, 18) + '...' : op.operation,
        cost: parseFloat(op.cost.toFixed(2)),
        percentage: parseFloat(op.percentage.toFixed(1)),
        color: getOperationColor(op.operation),
      }));
  }, [costMetrics.operationBreakdown]);

  return (
    <Box sx={{ width: '100%', height: '100%', overflow: 'auto', p: 2, bgcolor: '#0F172A' }}>
      <Grid container spacing={2}>
        <Grid item xs={12} sm={6} md={3}>
          <Card sx={{ bgcolor: '#1E293B', color: '#F1F5F9' }}>
            <CardContent>
              <Typography variant="body2" color="#94A3B8" gutterBottom>
                Total Cost
              </Typography>
              <Typography variant="h5">
                {costMetrics.totalCost.toFixed(2)}
              </Typography>
            </CardContent>
          </Card>
        </Grid>

        <Grid item xs={12} sm={6} md={3}>
          <Card sx={{ bgcolor: '#1E293B', color: '#F1F5F9' }}>
            <CardContent>
              <Typography variant="body2" color="#94A3B8" gutterBottom>
                Total Rows
              </Typography>
              <Typography variant="h5">
                {costMetrics.totalRows.toLocaleString()}
              </Typography>
            </CardContent>
          </Card>
        </Grid>

        <Grid item xs={12} sm={6} md={3}>
          <Card sx={{ bgcolor: '#1E293B', color: '#F1F5F9' }}>
            <CardContent>
              <Typography variant="body2" color="#94A3B8" gutterBottom>
                Plan Depth
              </Typography>
              <Typography variant="h5">
                {costMetrics.planDepth}
              </Typography>
            </CardContent>
          </Card>
        </Grid>

        <Grid item xs={12} sm={6} md={3}>
          <Card sx={{ bgcolor: '#1E293B', color: '#F1F5F9' }}>
            <CardContent>
              <Typography variant="body2" color="#94A3B8" gutterBottom>
                Operations
              </Typography>
              <Typography variant="h5">
                {costMetrics.operationBreakdown.length}
              </Typography>
            </CardContent>
          </Card>
        </Grid>

        <Grid item xs={12}>
          <Paper sx={{ bgcolor: '#1E293B', p: 2 }}>
            <Typography variant="h6" color="#F1F5F9" gutterBottom>
              Cost Distribution (Top 10)
            </Typography>
            <ResponsiveContainer width="100%" height={300}>
              <BarChart data={chartData} layout="vertical">
                <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
                <XAxis type="number" stroke="#94A3B8" />
                <YAxis dataKey="operation" type="category" width={150} stroke="#94A3B8" />
                <Tooltip
                  contentStyle={{ backgroundColor: '#1E293B', border: '1px solid #334155', color: '#F1F5F9' }}
                  formatter={(value: number, name: string) => {
                    if (name === 'cost') {
                      return [value.toFixed(2), 'Cost'];
                    }
                    return [value, name];
                  }}
                />
                <Bar dataKey="cost" radius={[0, 4, 4, 0]}>
                  {chartData.map((entry, index) => (
                    <Cell key={`cell-${index}`} fill={entry.color} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </Paper>
        </Grid>

        <Grid item xs={12}>
          <TableContainer component={Paper} sx={{ bgcolor: '#1E293B' }}>
            <Table size="small">
              <TableHead>
                <TableRow>
                  <TableCell sx={{ color: '#F1F5F9', fontWeight: 'bold' }}>
                    <TableSortLabel
                      active={sortField === 'operation'}
                      direction={sortField === 'operation' ? sortDirection : 'asc'}
                      onClick={() => handleSort('operation')}
                      sx={{
                        color: '#F1F5F9 !important',
                        '&.Mui-active': { color: '#60A5FA !important' },
                        '& .MuiTableSortLabel-icon': { color: '#60A5FA !important' },
                      }}
                    >
                      Operation
                    </TableSortLabel>
                  </TableCell>
                  <TableCell align="right" sx={{ color: '#F1F5F9', fontWeight: 'bold' }}>
                    <TableSortLabel
                      active={sortField === 'cost'}
                      direction={sortField === 'cost' ? sortDirection : 'asc'}
                      onClick={() => handleSort('cost')}
                      sx={{
                        color: '#F1F5F9 !important',
                        '&.Mui-active': { color: '#60A5FA !important' },
                        '& .MuiTableSortLabel-icon': { color: '#60A5FA !important' },
                      }}
                    >
                      Cost
                    </TableSortLabel>
                  </TableCell>
                  <TableCell align="right" sx={{ color: '#F1F5F9', fontWeight: 'bold' }}>
                    <TableSortLabel
                      active={sortField === 'rows'}
                      direction={sortField === 'rows' ? sortDirection : 'asc'}
                      onClick={() => handleSort('rows')}
                      sx={{
                        color: '#F1F5F9 !important',
                        '&.Mui-active': { color: '#60A5FA !important' },
                        '& .MuiTableSortLabel-icon': { color: '#60A5FA !important' },
                      }}
                    >
                      Rows
                    </TableSortLabel>
                  </TableCell>
                  <TableCell align="right" sx={{ color: '#F1F5F9', fontWeight: 'bold' }}>
                    <TableSortLabel
                      active={sortField === 'percentage'}
                      direction={sortField === 'percentage' ? sortDirection : 'asc'}
                      onClick={() => handleSort('percentage')}
                      sx={{
                        color: '#F1F5F9 !important',
                        '&.Mui-active': { color: '#60A5FA !important' },
                        '& .MuiTableSortLabel-icon': { color: '#60A5FA !important' },
                      }}
                    >
                      % of Total
                    </TableSortLabel>
                  </TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {sortedOperations.map((op, index) => (
                  <TableRow
                    key={index}
                    hover
                    sx={{
                      cursor: op.nodeId ? 'pointer' : 'default',
                      '&:hover': { bgcolor: '#334155' },
                    }}
                    onClick={() => {
                      if (op.nodeId && onNodeClick) {
                        onNodeClick(op.nodeId);
                      }
                    }}
                  >
                    <TableCell sx={{ color: '#F1F5F9' }}>
                      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                        <Box
                          sx={{
                            width: 12,
                            height: 12,
                            borderRadius: '50%',
                            bgcolor: getOperationColor(op.operation),
                          }}
                        />
                        {op.operation}
                      </Box>
                    </TableCell>
                    <TableCell align="right" sx={{ color: '#F1F5F9' }}>
                      {op.cost.toFixed(2)}
                    </TableCell>
                    <TableCell align="right" sx={{ color: '#F1F5F9' }}>
                      {op.rows.toLocaleString()}
                    </TableCell>
                    <TableCell align="right" sx={{ color: '#F1F5F9' }}>
                      {op.percentage.toFixed(1)}%
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableContainer>
        </Grid>
      </Grid>
    </Box>
  );
}

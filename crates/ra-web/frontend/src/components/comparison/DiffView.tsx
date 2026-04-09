import { useState, useCallback, useMemo } from 'react';
import {
  Box,
  Paper,
  Typography,
  Grid,
  Chip,
  IconButton,
  Tooltip,
} from '@mui/material';
import {
  ExpandMore as ExpandIcon,
  ChevronRight as CollapseIcon,
  UnfoldMore as ExpandAllIcon,
  UnfoldLess as CollapseAllIcon,
} from '@mui/icons-material';
import type { ParsedPlan, PlanNode } from '../../types';

interface DiffViewProps {
  plan1: ParsedPlan;
  plan2: ParsedPlan;
  onNodeClick?: (nodeId: string, planIndex: 1 | 2) => void;
}

type DiffStatus = 'added' | 'removed' | 'changed' | 'unchanged';

interface DiffNode {
  node1: PlanNode | null;
  node2: PlanNode | null;
  status: DiffStatus;
  depth: number;
}

interface CollapsedState {
  [key: string]: boolean;
}

const DIFF_COLORS = {
  added: { bg: 'rgba(34, 197, 94, 0.15)', border: '#22C55E' },
  removed: { bg: 'rgba(239, 68, 68, 0.15)', border: '#EF4444' },
  changed: { bg: 'rgba(251, 191, 36, 0.15)', border: '#FBBF24' },
  unchanged: { bg: 'transparent', border: '#64748B' },
};

function buildDiffTree(plan1: ParsedPlan, plan2: ParsedPlan): DiffNode[] {
  const diffNodes: DiffNode[] = [];
  const processedIds = new Set<string>();

  function traverse(nodeId1: string | null, nodeId2: string | null, depth: number): void {
    const node1 = nodeId1 ? plan1.nodes.find((n) => n.id === nodeId1) ?? null : null;
    const node2 = nodeId2 ? plan2.nodes.find((n) => n.id === nodeId2) ?? null : null;

    if (!node1 && !node2) {
      return;
    }

    const key = node1?.id ?? node2?.id ?? '';
    if (processedIds.has(key)) {
      return;
    }
    processedIds.add(key);

    let status: DiffStatus = 'unchanged';

    if (!node1 && node2) {
      status = 'added';
    } else if (node1 && !node2) {
      status = 'removed';
    } else if (node1 && node2) {
      const costDiff = Math.abs(node1.cost.total - node2.cost.total);
      const rowsDiff = Math.abs(node1.rows - node2.rows);
      const costThreshold = node1.cost.total * 0.1;
      const rowsThreshold = node1.rows * 0.1;

      if (costDiff > costThreshold || rowsDiff > rowsThreshold) {
        status = 'changed';
      }
    }

    diffNodes.push({ node1, node2, status, depth });

    const maxChildCount = Math.max(
      node1?.children.length ?? 0,
      node2?.children.length ?? 0
    );

    for (let i = 0; i < maxChildCount; i++) {
      const childId1 = node1?.children[i] ?? null;
      const childId2 = node2?.children[i] ?? null;
      traverse(childId1, childId2, depth + 1);
    }
  }

  traverse(plan1.rootNodeId, plan2.rootNodeId, 0);

  return diffNodes;
}

function formatCost(cost: number): string {
  if (cost >= 1000000) {
    return `${(cost / 1000000).toFixed(2)}M`;
  }
  if (cost >= 1000) {
    return `${(cost / 1000).toFixed(2)}K`;
  }
  return cost.toFixed(2);
}

function formatRows(rows: number): string {
  if (rows >= 1000000) {
    return `${(rows / 1000000).toFixed(2)}M`;
  }
  if (rows >= 1000) {
    return `${(rows / 1000).toFixed(2)}K`;
  }
  return rows.toString();
}

function calculateDiffPercentage(value1: number, value2: number): string {
  if (value1 === 0) {
    return '+∞%';
  }
  const diff = ((value2 - value1) / value1) * 100;
  const sign = diff > 0 ? '+' : '';
  return `${sign}${diff.toFixed(1)}%`;
}

export function DiffView({ plan1, plan2, onNodeClick }: DiffViewProps) {
  const [collapsed, setCollapsed] = useState<CollapsedState>({});
  const [allCollapsed, setAllCollapsed] = useState(false);

  const diffTree = useMemo(() => buildDiffTree(plan1, plan2), [plan1, plan2]);

  const toggleCollapse = useCallback((key: string) => {
    setCollapsed((prev) => ({
      ...prev,
      [key]: !prev[key],
    }));
  }, []);

  const expandAll = useCallback(() => {
    setCollapsed({});
    setAllCollapsed(false);
  }, []);

  const collapseAll = useCallback(() => {
    const newCollapsed: CollapsedState = {};
    diffTree.forEach((diffNode, index) => {
      if (diffNode.node1 || diffNode.node2) {
        newCollapsed[`diff-${index}`] = true;
      }
    });
    setCollapsed(newCollapsed);
    setAllCollapsed(true);
  }, [diffTree]);

  const isChildOfCollapsed = useCallback((index: number): boolean => {
    const currentDepth = diffTree[index]?.depth ?? 0;

    for (let i = index - 1; i >= 0; i--) {
      const node = diffTree[i];
      if (!node) {
        continue;
      }
      if (node.depth < currentDepth) {
        if (collapsed[`diff-${i}`]) {
          return true;
        }
      }
    }
    return false;
  }, [diffTree, collapsed]);

  const hasChildren = useCallback((index: number): boolean => {
    const currentDepth = diffTree[index]?.depth ?? 0;
    const nextNode = diffTree[index + 1];
    return nextNode ? nextNode.depth > currentDepth : false;
  }, [diffTree]);

  const renderNode = (node: PlanNode | null, side: 'left' | 'right', status: DiffStatus, otherNode: PlanNode | null) => {
    if (!node) {
      return (
        <Box
          sx={{
            p: 2,
            border: 1,
            borderRadius: 1,
            borderColor: DIFF_COLORS[status].border,
            bgcolor: DIFF_COLORS[status].bg,
            minHeight: 100,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            opacity: 0.5,
          }}
        >
          <Typography variant="body2" color="text.secondary">
            {status === 'added' ? 'Not in Plan 1' : 'Not in Plan 2'}
          </Typography>
        </Box>
      );
    }

    const planIndex = side === 'left' ? 1 : 2;
    const colors = DIFF_COLORS[status];

    return (
      <Box
        sx={{
          p: 2,
          border: 2,
          borderRadius: 1,
          borderColor: colors.border,
          bgcolor: colors.bg,
          cursor: onNodeClick ? 'pointer' : 'default',
          '&:hover': onNodeClick ? {
            boxShadow: 3,
            transform: 'translateY(-2px)',
            transition: 'all 0.2s',
          } : {},
        }}
        onClick={() => onNodeClick?.(node.id, planIndex)}
      >
        <Typography
          variant="subtitle2"
          sx={{
            fontWeight: 'bold',
            color: colors.border,
            mb: 1,
          }}
        >
          {node.operation}
        </Typography>

        {node.relation && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 1 }}>
            Table: {node.relation}
          </Typography>
        )}

        <Box sx={{ display: 'flex', gap: 1, flexWrap: 'wrap', mb: 1 }}>
          <Chip
            size="small"
            label={`Cost: ${formatCost(node.cost.total)}`}
            sx={{ bgcolor: 'rgba(147, 197, 253, 0.2)', fontSize: '0.7rem' }}
          />
          <Chip
            size="small"
            label={`Rows: ${formatRows(node.rows)}`}
            sx={{ bgcolor: 'rgba(167, 139, 250, 0.2)', fontSize: '0.7rem' }}
          />
        </Box>

        {status === 'changed' && otherNode && (
          <Box sx={{ mt: 1, pt: 1, borderTop: 1, borderColor: 'divider' }}>
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block' }}>
              Cost diff: {calculateDiffPercentage(
                side === 'left' ? node.cost.total : otherNode.cost.total,
                side === 'left' ? otherNode.cost.total : node.cost.total
              )}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block' }}>
              Rows diff: {calculateDiffPercentage(
                side === 'left' ? node.rows : otherNode.rows,
                side === 'left' ? otherNode.rows : node.rows
              )}
            </Typography>
          </Box>
        )}
      </Box>
    );
  };

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <Box
        sx={{
          display: 'flex',
          gap: 1,
          p: 1,
          borderBottom: 1,
          borderColor: 'divider',
          bgcolor: '#1E293B',
        }}
      >
        <Tooltip title="Expand all">
          <IconButton size="small" onClick={expandAll} disabled={!allCollapsed}>
            <ExpandAllIcon sx={{ color: '#F1F5F9' }} />
          </IconButton>
        </Tooltip>
        <Tooltip title="Collapse all">
          <IconButton size="small" onClick={collapseAll} disabled={allCollapsed}>
            <CollapseAllIcon sx={{ color: '#F1F5F9' }} />
          </IconButton>
        </Tooltip>

        <Box sx={{ flex: 1 }} />

        <Box sx={{ display: 'flex', gap: 2 }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <Box
              sx={{
                width: 16,
                height: 16,
                bgcolor: DIFF_COLORS.added.border,
                borderRadius: 0.5,
              }}
            />
            <Typography variant="caption" sx={{ color: '#F1F5F9' }}>Added</Typography>
          </Box>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <Box
              sx={{
                width: 16,
                height: 16,
                bgcolor: DIFF_COLORS.removed.border,
                borderRadius: 0.5,
              }}
            />
            <Typography variant="caption" sx={{ color: '#F1F5F9' }}>Removed</Typography>
          </Box>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <Box
              sx={{
                width: 16,
                height: 16,
                bgcolor: DIFF_COLORS.changed.border,
                borderRadius: 0.5,
              }}
            />
            <Typography variant="caption" sx={{ color: '#F1F5F9' }}>Changed</Typography>
          </Box>
        </Box>
      </Box>

      <Box
        sx={{
          flex: 1,
          overflow: 'auto',
          bgcolor: '#0F172A',
        }}
      >
        <Grid container sx={{ minHeight: '100%' }}>
          <Grid item xs={6} sx={{ borderRight: 1, borderColor: '#334155', p: 2 }}>
            <Paper elevation={0} sx={{ p: 2, mb: 2, bgcolor: '#1E293B', color: '#F1F5F9' }}>
              <Typography variant="h6">Plan 1</Typography>
              <Typography variant="caption" color="text.secondary">
                Total Cost: {formatCost(plan1.nodes.reduce((sum, n) => sum + n.cost.total, 0))}
              </Typography>
            </Paper>
          </Grid>
          <Grid item xs={6} sx={{ p: 2 }}>
            <Paper elevation={0} sx={{ p: 2, mb: 2, bgcolor: '#1E293B', color: '#F1F5F9' }}>
              <Typography variant="h6">Plan 2</Typography>
              <Typography variant="caption" color="text.secondary">
                Total Cost: {formatCost(plan2.nodes.reduce((sum, n) => sum + n.cost.total, 0))}
              </Typography>
            </Paper>
          </Grid>
        </Grid>

        {diffTree.map((diffNode, index) => {
          if (isChildOfCollapsed(index)) {
            return null;
          }

          const key = `diff-${index}`;
          const isCollapsed = collapsed[key] ?? false;
          const canCollapse = hasChildren(index);

          return (
            <Box key={key}>
              <Grid container spacing={2} sx={{ px: 2, mb: 2 }}>
                <Grid item xs={12}>
                  <Box
                    sx={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 1,
                      mb: 1,
                      pl: diffNode.depth * 3,
                    }}
                  >
                    <Box sx={{ width: 24, flexShrink: 0 }}>
                      {canCollapse && (
                        <IconButton
                          size="small"
                          onClick={() => toggleCollapse(key)}
                          sx={{ p: 0, color: '#F1F5F9' }}
                        >
                          {isCollapsed ? (
                            <CollapseIcon fontSize="small" />
                          ) : (
                            <ExpandIcon fontSize="small" />
                          )}
                        </IconButton>
                      )}
                    </Box>
                    <Chip
                      size="small"
                      label={diffNode.status.toUpperCase()}
                      sx={{
                        bgcolor: DIFF_COLORS[diffNode.status].border,
                        color: '#000',
                        fontWeight: 'bold',
                        fontSize: '0.7rem',
                      }}
                    />
                  </Box>
                </Grid>

                <Grid item xs={6}>
                  <Box sx={{ pl: diffNode.depth * 3 }}>
                    {renderNode(diffNode.node1, 'left', diffNode.status, diffNode.node2)}
                  </Box>
                </Grid>

                <Grid item xs={6}>
                  <Box sx={{ pl: diffNode.depth * 3 }}>
                    {renderNode(diffNode.node2, 'right', diffNode.status, diffNode.node1)}
                  </Box>
                </Grid>
              </Grid>
            </Box>
          );
        })}
      </Box>
    </Box>
  );
}

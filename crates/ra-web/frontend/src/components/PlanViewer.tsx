import { useState, useRef, useEffect } from 'react';
import {
  Box,
  Chip,
  IconButton,
  Tooltip,
  Typography,
} from '@mui/material';
import {
  ExpandMore as ExpandIcon,
  ChevronRight as CollapseIcon,
  UnfoldMore as ExpandAllIcon,
  UnfoldLess as CollapseAllIcon,
} from '@mui/icons-material';
import {
  parsePlan,
  parseTimingLine,
  formatTime,
  formatNumber,
  findMatches,
} from '../utils/planParser';

interface PlanViewerProps {
  planText: string;
  searchTerm: string;
  currentMatchIndex: number;
  onMatchCountChange: (count: number) => void;
}

interface CollapsedState {
  [lineIndex: number]: boolean;
}

export function PlanViewer({
  planText,
  searchTerm,
  currentMatchIndex,
  onMatchCountChange,
}: PlanViewerProps) {
  const [collapsed, setCollapsed] = useState<CollapsedState>({});
  const [allCollapsed, setAllCollapsed] = useState(false);
  const matchRefs = useRef<Array<HTMLSpanElement | null>>([]);
  const containerRef = useRef<HTMLDivElement>(null);

  const nodes = parsePlan(planText);
  const matches = findMatches(planText, searchTerm);

  useEffect(() => {
    onMatchCountChange(matches.length);
  }, [matches.length, onMatchCountChange]);

  useEffect(() => {
    if (matches.length > 0 && currentMatchIndex >= 0 && currentMatchIndex < matches.length) {
      const currentMatch = matches[currentMatchIndex];
      if (currentMatch) {
        const ref = matchRefs.current[currentMatchIndex];
        if (ref) {
          ref.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
      }
    }
  }, [currentMatchIndex, matches]);

  const toggleCollapse = (index: number) => {
    setCollapsed((prev) => ({
      ...prev,
      [index]: !prev[index],
    }));
  };

  const expandAll = () => {
    setCollapsed({});
    setAllCollapsed(false);
  };

  const collapseAll = () => {
    const newCollapsed: CollapsedState = {};
    nodes.forEach((node, index) => {
      if (node.operation) {
        newCollapsed[index] = true;
      }
    });
    setCollapsed(newCollapsed);
    setAllCollapsed(true);
  };

  const isChildOfCollapsed = (index: number): boolean => {
    const currentIndent = nodes[index]?.indentLevel ?? 0;

    for (let i = index - 1; i >= 0; i--) {
      const node = nodes[i];
      if (!node) {
        continue;
      }
      if (node.indentLevel < currentIndent) {
        if (collapsed[i]) {
          return true;
        }
        return isChildOfCollapsed(i);
      }
    }
    return false;
  };

  const hasChildren = (index: number): boolean => {
    const currentIndent = nodes[index]?.indentLevel ?? 0;
    const nextNode = nodes[index + 1];
    return nextNode ? nextNode.indentLevel > currentIndent : false;
  };

  const highlightLine = (line: string, lineIndex: number): JSX.Element => {
    if (!searchTerm) {
      return <>{formatLine(line)}</>;
    }

    const matchesInLine = matches.filter((m) => m.lineIndex === lineIndex);
    if (matchesInLine.length === 0) {
      return <>{formatLine(line)}</>;
    }

    const parts: JSX.Element[] = [];
    let lastIndex = 0;

    matchesInLine.forEach((match, idx) => {
      if (match.charIndex > lastIndex) {
        parts.push(
          <span key={`text-${idx}`}>
            {formatLine(line.substring(lastIndex, match.charIndex))}
          </span>
        );
      }

      const matchIndex = matches.indexOf(match);
      const isCurrentMatch = matchIndex === currentMatchIndex;

      parts.push(
        <span
          key={`match-${idx}`}
          ref={(el) => {
            matchRefs.current[matchIndex] = el;
          }}
          style={{
            backgroundColor: isCurrentMatch ? '#ff9632' : '#ffd700',
            color: '#000',
            padding: '0 2px',
            fontWeight: isCurrentMatch ? 'bold' : 'normal',
          }}
        >
          {line.substring(match.charIndex, match.charIndex + searchTerm.length)}
        </span>
      );

      lastIndex = match.charIndex + searchTerm.length;
    });

    if (lastIndex < line.length) {
      parts.push(
        <span key="text-end">{formatLine(line.substring(lastIndex))}</span>
      );
    }

    return <>{parts}</>;
  };

  const formatLine = (text: string): JSX.Element[] => {
    let remaining = text;

    const keywords = [
      'SELECT', 'FROM', 'WHERE', 'JOIN', 'ON', 'GROUP BY', 'ORDER BY',
      'HAVING', 'LIMIT', 'OFFSET', 'UNION', 'INTERSECT', 'EXCEPT',
    ];

    const operations = [
      'Seq Scan', 'Index Scan', 'Index Only Scan', 'Bitmap Heap Scan',
      'Bitmap Index Scan', 'Hash Join', 'Nested Loop', 'Merge Join',
      'Hash', 'Sort', 'Aggregate', 'Group', 'Filter', 'Limit',
      'Subquery Scan',
    ];

    // Highlight operation names
    for (const op of operations) {
      if (remaining.includes(op)) {
        const parts_temp = remaining.split(op);
        const result: JSX.Element[] = [];
        parts_temp.forEach((part, idx) => {
          if (idx > 0) {
            result.push(
              <span key={`op-${idx}`} style={{ color: '#4ec9b0', fontWeight: 'bold' }}>
                {op}
              </span>
            );
          }
          result.push(<span key={`text-${idx}`}>{part}</span>);
        });
        return result;
      }
    }

    // Highlight SQL keywords
    for (const keyword of keywords) {
      if (remaining.toUpperCase().includes(keyword)) {
        const regex = new RegExp(`\\b${keyword}\\b`, 'gi');
        const match = regex.exec(remaining);
        if (match) {
          const before = remaining.substring(0, match.index);
          const after = remaining.substring(match.index + keyword.length);
          return [
            <span key="before">{before}</span>,
            <span key="keyword" style={{ color: '#569cd6', fontWeight: 'bold' }}>
              {match[0]}
            </span>,
            <span key="after">{formatLine(after)}</span>,
          ];
        }
      }
    }

    // Highlight cost and timing info
    if (remaining.includes('cost=') || remaining.includes('rows=') || remaining.includes('width=')) {
      const regex = /(cost|rows|width|actual time|loops)=/gi;
      const match = regex.exec(remaining);
      if (match) {
        const before = remaining.substring(0, match.index);
        const keyword = match[0];
        const after = remaining.substring(match.index + (keyword?.length ?? 0));
        return [
          <span key="before">{formatLine(before)}</span>,
          <span key="metric" style={{ color: '#b5cea8' }}>
            {keyword}
          </span>,
          <span key="after">{formatLine(after)}</span>,
        ];
      }
    }

    // Highlight numbers
    if (/\d+\.?\d*/.test(remaining)) {
      const regex = /(\d+\.?\d*)/;
      const match = regex.exec(remaining);
      if (match) {
        const before = remaining.substring(0, match.index);
        const number = match[0];
        const after = remaining.substring(match.index + (number?.length ?? 0));
        return [
          <span key="before">{before}</span>,
          <span key="number" style={{ color: '#b5cea8' }}>
            {number}
          </span>,
          <span key="after">{formatLine(after)}</span>,
        ];
      }
    }

    return [<span key="text">{remaining}</span>];
  };

  return (
    <Box ref={containerRef} sx={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <Box
        sx={{
          display: 'flex',
          gap: 1,
          p: 1,
          borderBottom: 1,
          borderColor: 'divider',
        }}
      >
        <Tooltip title="Expand all">
          <IconButton size="small" onClick={expandAll} disabled={!allCollapsed}>
            <ExpandAllIcon />
          </IconButton>
        </Tooltip>
        <Tooltip title="Collapse all">
          <IconButton size="small" onClick={collapseAll} disabled={allCollapsed}>
            <CollapseAllIcon />
          </IconButton>
        </Tooltip>
      </Box>

      <Box
        sx={{
          flex: 1,
          overflow: 'auto',
          p: 2,
          fontFamily: 'monospace',
          fontSize: '0.875rem',
          bgcolor: '#1e1e1e',
          color: '#d4d4d4',
          lineHeight: 1.6,
        }}
      >
        {nodes.map((node, index) => {
          if (isChildOfCollapsed(index)) {
            return null;
          }

          const timing = parseTimingLine(node.line);
          const isCollapsed = collapsed[index] ?? false;
          const canCollapse = hasChildren(index);

          return (
            <Box
              key={index}
              sx={{
                display: 'flex',
                alignItems: 'flex-start',
                gap: 1,
                pl: node.indentLevel * 2,
                '&:hover': {
                  bgcolor: 'rgba(255, 255, 255, 0.05)',
                },
              }}
            >
              <Box sx={{ width: 20, flexShrink: 0 }}>
                {canCollapse && (
                  <IconButton
                    size="small"
                    onClick={() => toggleCollapse(index)}
                    sx={{ p: 0, color: 'inherit' }}
                  >
                    {isCollapsed ? <CollapseIcon fontSize="small" /> : <ExpandIcon fontSize="small" />}
                  </IconButton>
                )}
              </Box>

              <Box sx={{ flex: 1 }}>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, flexWrap: 'wrap' }}>
                  <Typography
                    component="span"
                    sx={{
                      fontFamily: 'inherit',
                      fontSize: 'inherit',
                      lineHeight: 'inherit',
                    }}
                  >
                    {highlightLine(node.line, index)}
                  </Typography>

                  {node.cost && (
                    <Chip
                      size="small"
                      label={`Cost: ${formatNumber(node.cost.total)}`}
                      sx={{
                        height: 20,
                        fontSize: '0.75rem',
                        bgcolor: 'rgba(181, 206, 168, 0.2)',
                        color: '#b5cea8',
                      }}
                    />
                  )}

                  {node.actual && (
                    <Chip
                      size="small"
                      label={`Time: ${formatTime(node.actual.time)}`}
                      sx={{
                        height: 20,
                        fontSize: '0.75rem',
                        bgcolor: 'rgba(255, 150, 50, 0.2)',
                        color: '#ff9632',
                      }}
                    />
                  )}

                  {timing && (
                    <Chip
                      size="small"
                      label={`${timing.label}: ${formatTime(timing.value)}`}
                      sx={{
                        height: 20,
                        fontSize: '0.75rem',
                        bgcolor: 'rgba(86, 156, 214, 0.2)',
                        color: '#569cd6',
                      }}
                    />
                  )}
                </Box>
              </Box>
            </Box>
          );
        })}
      </Box>
    </Box>
  );
}

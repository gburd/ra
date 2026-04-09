import { useState } from 'react';
import {
  Box,
  Card,
  CardContent,
  Typography,
  Chip,
  Alert,
  AlertTitle,
  Accordion,
  AccordionSummary,
  AccordionDetails,
  Stack,
} from '@mui/material';
import {
  ExpandMore as ExpandMoreIcon,
  Error as ErrorIcon,
  Warning as WarningIcon,
  Info as InfoIcon,
} from '@mui/icons-material';
import type { Warning } from '../../types';

interface WarningsViewProps {
  warnings: Warning[];
  onNodeClick?: (nodeId: string) => void;
}

function getSeverityColor(severity: Warning['severity']): 'error' | 'warning' | 'info' {
  return severity === 'critical' ? 'error' : severity;
}

function getSeverityIcon(severity: Warning['severity']) {
  switch (severity) {
    case 'critical':
      return <ErrorIcon />;
    case 'warning':
      return <WarningIcon />;
    case 'info':
      return <InfoIcon />;
    default:
      return <InfoIcon />;
  }
}

function getWarningTypeLabel(type: Warning['type']): string {
  const labels: Record<Warning['type'], string> = {
    full_table_scan: 'Full Table Scan',
    cartesian_product: 'Cartesian Product',
    missing_index: 'Missing Index',
    expensive_sort: 'Expensive Sort',
    inefficient_join: 'Inefficient Join',
    missing_statistics: 'Missing Statistics',
  };
  return labels[type] || type;
}

export function WarningsView({ warnings, onNodeClick }: WarningsViewProps) {
  const [expanded, setExpanded] = useState<string | false>(false);

  const handleChange = (panel: string) => (_event: React.SyntheticEvent, isExpanded: boolean) => {
    setExpanded(isExpanded ? panel : false);
  };

  const warningsByType = warnings.reduce((acc, warning) => {
    if (!acc[warning.type]) {
      acc[warning.type] = [];
    }
    acc[warning.type]!.push(warning);
    return acc;
  }, {} as Record<string, Warning[]>);

  const criticalCount = warnings.filter((w) => w.severity === 'critical').length;
  const warningCount = warnings.filter((w) => w.severity === 'warning').length;
  const infoCount = warnings.filter((w) => w.severity === 'info').length;

  if (warnings.length === 0) {
    return (
      <Box
        sx={{
          width: '100%',
          height: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          bgcolor: '#0F172A',
        }}
      >
        <Alert severity="success" sx={{ maxWidth: 600 }}>
          <AlertTitle>No Issues Detected</AlertTitle>
          The query plan looks good with no optimization warnings.
        </Alert>
      </Box>
    );
  }

  return (
    <Box sx={{ width: '100%', height: '100%', overflow: 'auto', p: 2, bgcolor: '#0F172A' }}>
      <Card sx={{ bgcolor: '#1E293B', mb: 2 }}>
        <CardContent>
          <Typography variant="h6" color="#F1F5F9" gutterBottom>
            Query Plan Analysis
          </Typography>
          <Stack direction="row" spacing={2}>
            {criticalCount > 0 && (
              <Chip
                icon={<ErrorIcon />}
                label={`${criticalCount} Critical`}
                color="error"
                size="small"
              />
            )}
            {warningCount > 0 && (
              <Chip
                icon={<WarningIcon />}
                label={`${warningCount} Warnings`}
                color="warning"
                size="small"
              />
            )}
            {infoCount > 0 && (
              <Chip
                icon={<InfoIcon />}
                label={`${infoCount} Info`}
                color="info"
                size="small"
              />
            )}
          </Stack>
        </CardContent>
      </Card>

      {Object.entries(warningsByType).map(([type, typeWarnings]) => (
        <Accordion
          key={type}
          expanded={expanded === type}
          onChange={handleChange(type)}
          sx={{
            bgcolor: '#1E293B',
            color: '#F1F5F9',
            mb: 1,
            '&:before': { display: 'none' },
          }}
        >
          <AccordionSummary
            expandIcon={<ExpandMoreIcon sx={{ color: '#94A3B8' }} />}
            sx={{
              '&:hover': { bgcolor: '#334155' },
            }}
          >
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 2, width: '100%' }}>
              {getSeverityIcon(typeWarnings[0]!.severity)}
              <Typography sx={{ flex: 1 }}>
                {getWarningTypeLabel(type as Warning['type'])}
              </Typography>
              <Chip
                label={`${typeWarnings.length} issue${typeWarnings.length > 1 ? 's' : ''}`}
                size="small"
                color={getSeverityColor(typeWarnings[0]!.severity)}
              />
            </Box>
          </AccordionSummary>
          <AccordionDetails>
            <Stack spacing={2}>
              {typeWarnings.map((warning, index) => (
                <Alert
                  key={`${warning.nodeId}-${index}`}
                  severity={getSeverityColor(warning.severity)}
                  sx={{
                    bgcolor: '#0F172A',
                    cursor: 'pointer',
                    '&:hover': {
                      bgcolor: '#1E293B',
                    },
                  }}
                  onClick={() => {
                    if (onNodeClick) {
                      onNodeClick(warning.nodeId);
                    }
                  }}
                >
                  <AlertTitle>{warning.message}</AlertTitle>
                  <Typography variant="body2" gutterBottom>
                    {warning.suggestion}
                  </Typography>
                  <Typography variant="caption" color="text.secondary">
                    Node ID: {warning.nodeId}
                  </Typography>
                </Alert>
              ))}
            </Stack>
          </AccordionDetails>
        </Accordion>
      ))}
    </Box>
  );
}

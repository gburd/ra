import { useState } from 'react';
import {
  Box,
  Paper,
  Typography,
  IconButton,
  Tooltip,
  CircularProgress,
  Alert,
  TextField,
} from '@mui/material';
import {
  ContentCopy as CopyIcon,
  Search as SearchIcon,
} from '@mui/icons-material';
import { EngineSelector } from './EngineSelector';
import type { OutputPanelState, Engine } from '../types';

interface OutputPanelProps {
  panel: OutputPanelState;
  onEngineChange: (panelId: string, engine: Engine) => void;
}

export function OutputPanel({ panel, onEngineChange }: OutputPanelProps) {
  const [searchTerm, setSearchTerm] = useState('');
  const [copySuccess, setCopySuccess] = useState(false);

  const handleCopy = async () => {
    if (panel.output) {
      await navigator.clipboard.writeText(panel.output);
      setCopySuccess(true);
      setTimeout(() => setCopySuccess(false), 2000);
    }
  };

  const handleEngineChange = (engine: Engine) => {
    onEngineChange(panel.id, engine);
  };

  const getHighlightedOutput = (text: string, search: string): string => {
    if (!search) return text;

    const regex = new RegExp(`(${search})`, 'gi');
    return text.replace(regex, '<mark>$1</mark>');
  };

  return (
    <Paper
      elevation={2}
      sx={{
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      <Box
        sx={{
          p: 1.5,
          borderBottom: 1,
          borderColor: 'divider',
          display: 'flex',
          alignItems: 'center',
          gap: 1,
        }}
      >
        <Box sx={{ flex: 1 }}>
          <EngineSelector
            value={panel.engine}
            onChange={handleEngineChange}
            label="Engine"
          />
        </Box>

        <Tooltip title="Search">
          <IconButton size="small">
            <SearchIcon />
          </IconButton>
        </Tooltip>

        <Tooltip title={copySuccess ? 'Copied!' : 'Copy to clipboard'}>
          <span>
            <IconButton
              size="small"
              onClick={handleCopy}
              disabled={!panel.output}
            >
              <CopyIcon />
            </IconButton>
          </span>
        </Tooltip>
      </Box>

      {searchTerm && (
        <Box sx={{ px: 1.5, pt: 1 }}>
          <TextField
            size="small"
            fullWidth
            placeholder="Search in output..."
            value={searchTerm}
            onChange={e => setSearchTerm(e.target.value)}
            InputProps={{
              startAdornment: <SearchIcon sx={{ mr: 1, color: 'text.secondary' }} />,
            }}
          />
        </Box>
      )}

      <Box
        sx={{
          flex: 1,
          overflow: 'auto',
          p: 2,
          fontFamily: 'monospace',
          fontSize: '0.875rem',
          bgcolor: '#1e1e1e',
          color: '#d4d4d4',
        }}
      >
        {panel.loading && (
          <Box
            sx={{
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              height: '100%',
              gap: 2,
            }}
          >
            <CircularProgress />
            <Typography color="text.secondary">
              Executing query...
            </Typography>
          </Box>
        )}

        {panel.error && (
          <Alert severity="error" sx={{ fontFamily: 'inherit' }}>
            <Typography
              component="pre"
              sx={{ fontFamily: 'inherit', whiteSpace: 'pre-wrap', m: 0 }}
            >
              {panel.error}
            </Typography>
          </Alert>
        )}

        {panel.output && !panel.loading && (
          <Box
            component="pre"
            sx={{
              m: 0,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              '& mark': {
                backgroundColor: '#ffd700',
                color: '#000',
              },
            }}
            dangerouslySetInnerHTML={{
              __html: getHighlightedOutput(panel.output, searchTerm),
            }}
          />
        )}

        {!panel.loading && !panel.error && !panel.output && (
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              height: '100%',
            }}
          >
            <Typography color="text.secondary">
              No output yet. Run a query to see results.
            </Typography>
          </Box>
        )}
      </Box>
    </Paper>
  );
}

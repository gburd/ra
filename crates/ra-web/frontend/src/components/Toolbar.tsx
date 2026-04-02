import {
  AppBar,
  Toolbar as MuiToolbar,
  Typography,
  Button,
  ToggleButtonGroup,
  ToggleButton,
  Box,
  IconButton,
  Tooltip,
  Menu,
  MenuItem,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogContentText,
  DialogActions,
  TextField,
} from '@mui/material';
import {
  PlayArrow as PlayIcon,
  Add as AddIcon,
  Share as ShareIcon,
  TableChart as SchemaIcon,
  Menu as MenuIcon,
} from '@mui/icons-material';
import { useState } from 'react';
import type { ExplainMode } from '../types';

interface ToolbarProps {
  explainMode: ExplainMode;
  onExplainModeChange: (mode: ExplainMode) => void;
  onExecute: () => void;
  onAddPanel: () => void;
  onShare: () => void;
  onShowSchemas: () => void;
  canAddPanel: boolean;
  executing: boolean;
}

export function Toolbar({
  explainMode,
  onExplainModeChange,
  onExecute,
  onAddPanel,
  onShare,
  onShowSchemas,
  canAddPanel,
  executing,
}: ToolbarProps) {
  const [menuAnchor, setMenuAnchor] = useState<null | HTMLElement>(null);

  const handleMenuOpen = (event: React.MouseEvent<HTMLElement>) => {
    setMenuAnchor(event.currentTarget);
  };

  const handleMenuClose = () => {
    setMenuAnchor(null);
  };

  const handleExplainModeChange = (
    _event: React.MouseEvent<HTMLElement>,
    newMode: ExplainMode | null
  ) => {
    if (newMode !== null) {
      onExplainModeChange(newMode);
    }
  };

  return (
    <AppBar position="static" color="default" elevation={1}>
      <MuiToolbar variant="dense">
        <Typography variant="h6" component="div" sx={{ mr: 2 }}>
          RA SQL Optimizer
        </Typography>

        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, flex: 1 }}>
          <Button
            variant="contained"
            color="primary"
            startIcon={<PlayIcon />}
            onClick={onExecute}
            disabled={executing}
          >
            Execute
          </Button>

          <ToggleButtonGroup
            value={explainMode}
            exclusive
            onChange={handleExplainModeChange}
            size="small"
          >
            <ToggleButton value="explain">EXPLAIN</ToggleButton>
            <ToggleButton value="analyze">EXPLAIN ANALYZE</ToggleButton>
          </ToggleButtonGroup>

          <Box sx={{ flex: 1 }} />

          <Tooltip title="Add engine panel">
            <span>
              <IconButton
                onClick={onAddPanel}
                disabled={!canAddPanel}
                color="primary"
              >
                <AddIcon />
              </IconButton>
            </span>
          </Tooltip>

          <Tooltip title="View schemas">
            <IconButton onClick={onShowSchemas} color="primary">
              <SchemaIcon />
            </IconButton>
          </Tooltip>

          <Tooltip title="Share query">
            <IconButton onClick={onShare} color="primary">
              <ShareIcon />
            </IconButton>
          </Tooltip>

          <IconButton onClick={handleMenuOpen}>
            <MenuIcon />
          </IconButton>

          <Menu
            anchorEl={menuAnchor}
            open={Boolean(menuAnchor)}
            onClose={handleMenuClose}
          >
            <MenuItem onClick={() => { onShowSchemas(); handleMenuClose(); }}>
              View Schemas
            </MenuItem>
            <MenuItem onClick={() => { onShare(); handleMenuClose(); }}>
              Share Query
            </MenuItem>
          </Menu>
        </Box>
      </MuiToolbar>
    </AppBar>
  );
}

interface ShareDialogProps {
  open: boolean;
  url: string;
  onClose: () => void;
}

export function ShareDialog({ open, url, onClose }: ShareDialogProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>Share Query</DialogTitle>
      <DialogContent>
        <DialogContentText sx={{ mb: 2 }}>
          Copy this URL to share your query with others:
        </DialogContentText>
        <TextField
          fullWidth
          value={url}
          InputProps={{
            readOnly: true,
          }}
          onClick={e => (e.target as HTMLInputElement).select()}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
        <Button onClick={handleCopy} variant="contained">
          {copied ? 'Copied!' : 'Copy URL'}
        </Button>
      </DialogActions>
    </Dialog>
  );
}

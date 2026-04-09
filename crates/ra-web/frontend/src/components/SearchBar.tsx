import { useState, useEffect } from 'react';
import {
  TextField,
  IconButton,
  Typography,
  Paper,
} from '@mui/material';
import {
  KeyboardArrowUp as PrevIcon,
  KeyboardArrowDown as NextIcon,
  Close as CloseIcon,
} from '@mui/icons-material';

interface SearchBarProps {
  onSearch: (term: string) => void;
  onNavigate: (direction: 'prev' | 'next') => void;
  onClose: () => void;
  matchCount: number;
  currentMatch: number;
}

export function SearchBar({
  onSearch,
  onNavigate,
  onClose,
  matchCount,
  currentMatch,
}: SearchBarProps) {
  const [term, setTerm] = useState('');

  useEffect(() => {
    onSearch(term);
  }, [term, onSearch]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.shiftKey) {
        onNavigate('prev');
      } else {
        onNavigate('next');
      }
    }
    if (e.key === 'Escape') {
      onClose();
    }
  };

  return (
    <Paper
      elevation={1}
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 1,
        px: 1.5,
        py: 1,
        borderBottom: 1,
        borderColor: 'divider',
      }}
    >
      <TextField
        autoFocus
        size="small"
        placeholder="Search in plan..."
        value={term}
        onChange={(e) => setTerm(e.target.value)}
        onKeyDown={handleKeyDown}
        sx={{ flex: 1, minWidth: 200 }}
      />

      {term && (
        <Typography variant="caption" color="text.secondary" sx={{ px: 1 }}>
          {matchCount === 0
            ? 'No matches'
            : `${currentMatch + 1} of ${matchCount}`}
        </Typography>
      )}

      <IconButton
        size="small"
        onClick={() => onNavigate('prev')}
        disabled={matchCount === 0}
        title="Previous match (Shift+Enter)"
      >
        <PrevIcon />
      </IconButton>

      <IconButton
        size="small"
        onClick={() => onNavigate('next')}
        disabled={matchCount === 0}
        title="Next match (Enter)"
      >
        <NextIcon />
      </IconButton>

      <IconButton size="small" onClick={onClose} title="Close (Esc)">
        <CloseIcon />
      </IconButton>
    </Paper>
  );
}

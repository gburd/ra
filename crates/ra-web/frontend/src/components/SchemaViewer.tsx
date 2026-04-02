import {
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  Button,
  Tabs,
  Tab,
  Box,
  Typography,
  List,
  ListItem,
  ListItemButton,
  ListItemText,
  Paper,
  Divider,
} from '@mui/material';
import { useState } from 'react';
import { SCHEMAS } from '../constants';
import type { Schema, SampleQuery } from '../types';

interface SchemaViewerProps {
  open: boolean;
  onClose: () => void;
  onLoadQuery: (sql: string) => void;
}

export function SchemaViewer({ open, onClose, onLoadQuery }: SchemaViewerProps) {
  const [selectedSchema, setSelectedSchema] = useState(0);
  const [selectedTab, setSelectedTab] = useState<'tables' | 'queries'>('tables');

  const schema = SCHEMAS[selectedSchema];

  const handleLoadQuery = (query: SampleQuery) => {
    onLoadQuery(query.sql);
    onClose();
  };

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth>
      <DialogTitle>Database Schemas</DialogTitle>
      <DialogContent>
        <Tabs
          value={selectedSchema}
          onChange={(_e, v) => setSelectedSchema(v as number)}
          sx={{ borderBottom: 1, borderColor: 'divider', mb: 2 }}
        >
          {SCHEMAS.map((schema, index) => (
            <Tab key={schema.name} label={schema.name} value={index} />
          ))}
        </Tabs>

        {schema && (
          <>
            <Tabs
              value={selectedTab}
              onChange={(_e, v) => setSelectedTab(v as 'tables' | 'queries')}
              sx={{ mb: 2 }}
            >
              <Tab label="Tables" value="tables" />
              <Tab label="Sample Queries" value="queries" />
            </Tabs>

            {selectedTab === 'tables' && (
              <Box sx={{ maxHeight: 400, overflow: 'auto' }}>
                {schema.tables.map((table, index) => (
                  <Box key={table.name} sx={{ mb: 2 }}>
                    <Typography variant="h6" sx={{ mb: 1 }}>
                      {table.name}
                    </Typography>
                    <Paper
                      variant="outlined"
                      sx={{
                        p: 2,
                        bgcolor: '#1e1e1e',
                        color: '#d4d4d4',
                        fontFamily: 'monospace',
                        fontSize: '0.875rem',
                      }}
                    >
                      <pre style={{ margin: 0 }}>{table.ddl}</pre>
                    </Paper>
                    {index < schema.tables.length - 1 && (
                      <Divider sx={{ my: 2 }} />
                    )}
                  </Box>
                ))}
              </Box>
            )}

            {selectedTab === 'queries' && (
              <List sx={{ maxHeight: 400, overflow: 'auto' }}>
                {schema.sampleQueries.map(query => (
                  <ListItem key={query.name} disablePadding>
                    <ListItemButton
                      onClick={() => handleLoadQuery(query)}
                    >
                      <ListItemText
                        primary={query.name}
                        secondary={query.description}
                      />
                    </ListItemButton>
                  </ListItem>
                ))}
              </List>
            )}
          </>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
  );
}

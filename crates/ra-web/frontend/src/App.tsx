import { useState, useCallback, useEffect } from 'react';
import { Box, CssBaseline, ThemeProvider, createTheme } from '@mui/material';
import { Allotment } from 'allotment';
import 'allotment/dist/style.css';
import { Editor } from './components/Editor';
import { OutputPanel } from './components/OutputPanel';
import { Toolbar, ShareDialog } from './components/Toolbar';
import { SchemaViewer } from './components/SchemaViewer';
import { useQueryExecution } from './hooks/useQueryExecution';
import { generateShareUrl, getStateFromUrl } from './utils/urlEncoding';
import { DEFAULT_ENGINE, DEFAULT_SQL } from './constants';
import type { AppState, Engine, ExplainMode, OutputPanelState, VisualizationTab } from './types';

const theme = createTheme({
  palette: {
    mode: 'dark',
    primary: {
      main: '#667eea',
    },
  },
});

const MAX_PANELS = 4;

function App() {
  const [state, setState] = useState<AppState>(() => {
    const urlState = getStateFromUrl();
    return {
      sql: urlState?.sql || DEFAULT_SQL,
      explainMode: urlState?.explainMode || 'explain',
      panels: urlState?.panels || [
        {
          id: 'panel-0',
          engine: DEFAULT_ENGINE,
          output: null,
          rawPlan: null,
          parsedPlan: null,
          costMetrics: null,
          warnings: null,
          loading: false,
          error: null,
          activeTab: 'raw' as VisualizationTab,
        },
      ],
    };
  });

  const [shareDialogOpen, setShareDialogOpen] = useState(false);
  const [shareUrl, setShareUrl] = useState('');
  const [schemaViewerOpen, setSchemaViewerOpen] = useState(false);
  const [highlightedNodeId, setHighlightedNodeId] = useState<string | undefined>(undefined);

  const updatePanel = useCallback(
    (panelId: string, updates: Partial<OutputPanelState>) => {
      setState(prevState => ({
        ...prevState,
        panels: prevState.panels.map(panel =>
          panel.id === panelId ? { ...panel, ...updates } : panel
        ),
      }));
    },
    []
  );

  const { executeAllPanels } = useQueryExecution(updatePanel);

  const handleSqlChange = (sql: string) => {
    setState(prevState => ({ ...prevState, sql }));
  };

  const handleExplainModeChange = (explainMode: ExplainMode) => {
    setState(prevState => ({ ...prevState, explainMode }));
  };

  const handleExecute = useCallback(() => {
    void executeAllPanels(state.panels, state.sql, state.explainMode);
  }, [executeAllPanels, state.panels, state.sql, state.explainMode]);

  const handleAddPanel = () => {
    if (state.panels.length < MAX_PANELS) {
      const newPanel: OutputPanelState = {
        id: `panel-${state.panels.length}`,
        engine: DEFAULT_ENGINE,
        output: null,
        rawPlan: null,
        parsedPlan: null,
        costMetrics: null,
        warnings: null,
        loading: false,
        error: null,
        activeTab: 'raw' as VisualizationTab,
      };

      setState(prevState => ({
        ...prevState,
        panels: [...prevState.panels, newPanel],
      }));
    }
  };

  const handleEngineChange = (panelId: string, engine: Engine) => {
    updatePanel(panelId, { engine });
  };

  const handleShare = () => {
    const url = generateShareUrl(state);
    setShareUrl(url);
    setShareDialogOpen(true);
  };

  const handleLoadQuery = (sql: string) => {
    setState(prevState => ({ ...prevState, sql }));
  };

  const handleNodeHighlight = (nodeId: string) => {
    setHighlightedNodeId(nodeId);
  };

  const handleTabChange = (panelId: string, tab: VisualizationTab) => {
    updatePanel(panelId, { activeTab: tab });
  };

  const isExecuting = state.panels.some(panel => panel.loading);

  useEffect(() => {
    const handlePopState = () => {
      const urlState = getStateFromUrl();
      if (urlState) {
        setState(prevState => ({
          ...prevState,
          ...urlState,
        }));
      }
    };

    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, []);

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <Box
        sx={{
          height: '100vh',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        <Toolbar
          explainMode={state.explainMode}
          onExplainModeChange={handleExplainModeChange}
          onExecute={handleExecute}
          onAddPanel={handleAddPanel}
          onShare={handleShare}
          onShowSchemas={() => setSchemaViewerOpen(true)}
          canAddPanel={state.panels.length < MAX_PANELS}
          executing={isExecuting}
        />

        <Box sx={{ flex: 1, overflow: 'hidden' }}>
          <Allotment defaultSizes={[40, 60]}>
            <Allotment.Pane minSize={300}>
              <Box sx={{ height: '100%', bgcolor: 'background.paper' }}>
                <Editor
                  value={state.sql}
                  onChange={handleSqlChange}
                  onExecute={handleExecute}
                />
              </Box>
            </Allotment.Pane>

            <Allotment.Pane minSize={300}>
              {state.panels.length === 1 ? (
                <Box sx={{ height: '100%', p: 1 }}>
                  <OutputPanel
                    panel={state.panels[0]!}
                    onEngineChange={handleEngineChange}
                    highlightedNodeId={highlightedNodeId}
                    onNodeHighlight={handleNodeHighlight}
                    onTabChange={handleTabChange}
                  />
                </Box>
              ) : (
                <Allotment vertical>
                  {state.panels.map(panel => (
                    <Allotment.Pane key={panel.id}>
                      <Box sx={{ height: '100%', p: 1 }}>
                        <OutputPanel
                          panel={panel}
                          onEngineChange={handleEngineChange}
                          highlightedNodeId={highlightedNodeId}
                          onNodeHighlight={handleNodeHighlight}
                          onTabChange={handleTabChange}
                        />
                      </Box>
                    </Allotment.Pane>
                  ))}
                </Allotment>
              )}
            </Allotment.Pane>
          </Allotment>
        </Box>

        <ShareDialog
          open={shareDialogOpen}
          url={shareUrl}
          onClose={() => setShareDialogOpen(false)}
        />

        <SchemaViewer
          open={schemaViewerOpen}
          onClose={() => setSchemaViewerOpen(false)}
          onLoadQuery={handleLoadQuery}
        />
      </Box>
    </ThemeProvider>
  );
}

export default App;

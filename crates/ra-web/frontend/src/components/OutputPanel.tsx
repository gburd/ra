import { useState, useMemo, lazy, Suspense } from 'react';
import {
  Box,
  Paper,
  Typography,
  IconButton,
  Tooltip,
  CircularProgress,
  Alert,
  Snackbar,
  Tabs,
  Tab,
} from '@mui/material';
import {
  ContentCopy as CopyIcon,
  Search as SearchIcon,
} from '@mui/icons-material';
import { EngineSelector } from './EngineSelector';
import { PlanViewer } from './PlanViewer';
import { SearchBar } from './SearchBar';
import { parsePlan } from '../parsers';
import { extractCostMetrics, detectWarnings } from '../utils/warningDetector';
import type { OutputPanelState, Engine, VisualizationTab } from '../types';

const PlanTreeView = lazy(() =>
  import('./visualizations/PlanTreeView').then((module) => ({
    default: module.PlanTreeView,
  }))
);
const PlanFlowView = lazy(() =>
  import('./visualizations/PlanFlowView').then((module) => ({
    default: module.PlanFlowView,
  }))
);
const CostAnalysisView = lazy(() =>
  import('./visualizations/CostAnalysisView').then((module) => ({
    default: module.CostAnalysisView,
  }))
);
const WarningsView = lazy(() =>
  import('./visualizations/WarningsView').then((module) => ({
    default: module.WarningsView,
  }))
);

interface OutputPanelProps {
  panel: OutputPanelState;
  onEngineChange: (panelId: string, engine: Engine) => void;
  highlightedNodeId: string | undefined;
  onNodeHighlight: ((nodeId: string) => void) | undefined;
  onTabChange?: (panelId: string, tab: VisualizationTab) => void;
}

export function OutputPanel({
  panel,
  onEngineChange,
  highlightedNodeId,
  onNodeHighlight,
  onTabChange,
}: OutputPanelProps) {
  const [searchVisible, setSearchVisible] = useState(false);
  const [searchTerm, setSearchTerm] = useState('');
  const [matchCount, setMatchCount] = useState(0);
  const [currentMatchIndex, setCurrentMatchIndex] = useState(0);
  const [copySuccess, setCopySuccess] = useState(false);

  const parsedPlan = useMemo(() => {
    return parsePlan(panel.output, panel.engine);
  }, [panel.output, panel.engine]);

  const costMetrics = useMemo(() => {
    if (!parsedPlan) {
      return null;
    }
    return extractCostMetrics(parsedPlan);
  }, [parsedPlan]);

  const warnings = useMemo(() => {
    if (!parsedPlan) {
      return [];
    }
    return detectWarnings(parsedPlan);
  }, [parsedPlan]);

  const handleCopy = async () => {
    if (panel.output) {
      await navigator.clipboard.writeText(panel.output);
      setCopySuccess(true);
    }
  };

  const handleEngineChange = (engine: Engine) => {
    onEngineChange(panel.id, engine);
  };

  const handleSearch = (term: string) => {
    setSearchTerm(term);
    setCurrentMatchIndex(0);
  };

  const handleNavigate = (direction: 'prev' | 'next') => {
    if (matchCount === 0) {
      return;
    }

    if (direction === 'next') {
      setCurrentMatchIndex((prev) => (prev + 1) % matchCount);
    } else {
      setCurrentMatchIndex((prev) => (prev - 1 + matchCount) % matchCount);
    }
  };

  const handleSearchClose = () => {
    setSearchVisible(false);
    setSearchTerm('');
    setCurrentMatchIndex(0);
  };

  const handleTabChange = (_event: React.SyntheticEvent, newValue: VisualizationTab) => {
    if (onTabChange) {
      onTabChange(panel.id, newValue);
    }
  };

  const handleNodeClick = (nodeId: string) => {
    if (onNodeHighlight) {
      onNodeHighlight(nodeId);
    }
  };

  const activeTab = panel.activeTab ?? 'raw';

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

        {activeTab === 'raw' && (
          <>
            <Tooltip title="Search">
              <IconButton
                size="small"
                onClick={() => setSearchVisible(!searchVisible)}
                color={searchVisible ? 'primary' : 'default'}
              >
                <SearchIcon />
              </IconButton>
            </Tooltip>

            <Tooltip title="Copy to clipboard">
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
          </>
        )}
      </Box>

      {panel.output && !panel.loading && (
        <Box sx={{ borderBottom: 1, borderColor: 'divider' }}>
          <Tabs
            value={activeTab}
            onChange={handleTabChange}
            variant="scrollable"
            scrollButtons="auto"
          >
            <Tab label="Raw Plan" value="raw" />
            <Tab label="Tree View" value="tree" disabled={!parsedPlan} />
            <Tab label="Flow View" value="flow" disabled={!parsedPlan} />
            <Tab label="Cost Analysis" value="cost" disabled={!costMetrics} />
            <Tab
              label={
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
                  Warnings
                  {warnings.length > 0 && (
                    <Box
                      component="span"
                      sx={{
                        bgcolor: 'error.main',
                        color: 'white',
                        borderRadius: '50%',
                        width: 20,
                        height: 20,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        fontSize: 11,
                        fontWeight: 'bold',
                      }}
                    >
                      {warnings.length}
                    </Box>
                  )}
                </Box>
              }
              value="warnings"
            />
          </Tabs>
        </Box>
      )}

      {searchVisible && activeTab === 'raw' && (
        <SearchBar
          onSearch={handleSearch}
          onNavigate={handleNavigate}
          onClose={handleSearchClose}
          matchCount={matchCount}
          currentMatch={currentMatchIndex}
        />
      )}

      <Box
        sx={{
          flex: 1,
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
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
          <Alert severity="error" sx={{ m: 2, fontFamily: 'monospace' }}>
            <Typography
              component="pre"
              sx={{ fontFamily: 'inherit', whiteSpace: 'pre-wrap', m: 0 }}
            >
              {panel.error}
            </Typography>
          </Alert>
        )}

        {panel.output && !panel.loading && (
          <>
            {activeTab === 'raw' && (
              <PlanViewer
                planText={panel.output}
                searchTerm={searchTerm}
                currentMatchIndex={currentMatchIndex}
                onMatchCountChange={setMatchCount}
              />
            )}

            {activeTab === 'tree' && parsedPlan && (
              <Suspense
                fallback={
                  <Box
                    sx={{
                      display: 'flex',
                      justifyContent: 'center',
                      alignItems: 'center',
                      height: '100%',
                    }}
                  >
                    <CircularProgress />
                  </Box>
                }
              >
                <PlanTreeView
                  parsedPlan={parsedPlan}
                  highlightedNodeId={highlightedNodeId}
                  onNodeClick={handleNodeClick}
                />
              </Suspense>
            )}

            {activeTab === 'flow' && parsedPlan && (
              <Suspense
                fallback={
                  <Box
                    sx={{
                      display: 'flex',
                      justifyContent: 'center',
                      alignItems: 'center',
                      height: '100%',
                    }}
                  >
                    <CircularProgress />
                  </Box>
                }
              >
                <PlanFlowView
                  parsedPlan={parsedPlan}
                  highlightedNodeId={highlightedNodeId}
                  onNodeClick={handleNodeClick}
                />
              </Suspense>
            )}

            {activeTab === 'cost' && costMetrics && (
              <Suspense
                fallback={
                  <Box
                    sx={{
                      display: 'flex',
                      justifyContent: 'center',
                      alignItems: 'center',
                      height: '100%',
                    }}
                  >
                    <CircularProgress />
                  </Box>
                }
              >
                <CostAnalysisView
                  costMetrics={costMetrics}
                  onNodeClick={handleNodeClick}
                />
              </Suspense>
            )}

            {activeTab === 'warnings' && (
              <Suspense
                fallback={
                  <Box
                    sx={{
                      display: 'flex',
                      justifyContent: 'center',
                      alignItems: 'center',
                      height: '100%',
                    }}
                  >
                    <CircularProgress />
                  </Box>
                }
              >
                <WarningsView
                  warnings={warnings}
                  onNodeClick={handleNodeClick}
                />
              </Suspense>
            )}
          </>
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

      <Snackbar
        open={copySuccess}
        autoHideDuration={2000}
        onClose={() => setCopySuccess(false)}
        message="Copied to clipboard"
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      />
    </Paper>
  );
}

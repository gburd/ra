import { useState, useCallback } from 'react';
import { executeQuery, ApiError } from '../utils/api';
import type { Engine, ExplainMode, OutputPanelState } from '../types';

interface UseQueryExecutionResult {
  executeSinglePanel: (
    panelId: string,
    sql: string,
    engine: Engine,
    explainMode: ExplainMode
  ) => Promise<void>;
  executeAllPanels: (
    panels: OutputPanelState[],
    sql: string,
    explainMode: ExplainMode
  ) => Promise<void>;
}

export function useQueryExecution(
  updatePanel: (panelId: string, updates: Partial<OutputPanelState>) => void
): UseQueryExecutionResult {
  const [abortControllers] = useState(new Map<string, AbortController>());

  const executeSinglePanel = useCallback(
    async (
      panelId: string,
      sql: string,
      engine: Engine,
      explainMode: ExplainMode
    ): Promise<void> => {
      const existingController = abortControllers.get(panelId);
      if (existingController) {
        existingController.abort();
      }

      const controller = new AbortController();
      abortControllers.set(panelId, controller);

      updatePanel(panelId, { loading: true, error: null, output: null });

      try {
        const engineName = engine.split('-')[0];
        if (!engineName) {
          throw new Error('Invalid engine format');
        }

        const response = await executeQuery({
          sql,
          engine: engineName,
          analyze: explainMode === 'analyze',
        });

        if (!controller.signal.aborted) {
          updatePanel(panelId, {
            loading: false,
            output: response.plan,
            error: null,
          });
        }
      } catch (error) {
        if (!controller.signal.aborted) {
          const errorMessage =
            error instanceof ApiError
              ? error.message
              : error instanceof Error
              ? error.message
              : 'Unknown error occurred';

          updatePanel(panelId, {
            loading: false,
            error: errorMessage,
            output: null,
          });
        }
      } finally {
        abortControllers.delete(panelId);
      }
    },
    [abortControllers, updatePanel]
  );

  const executeAllPanels = useCallback(
    async (
      panels: OutputPanelState[],
      sql: string,
      explainMode: ExplainMode
    ): Promise<void> => {
      await Promise.all(
        panels.map(panel =>
          executeSinglePanel(panel.id, sql, panel.engine, explainMode)
        )
      );
    },
    [executeSinglePanel]
  );

  return {
    executeSinglePanel,
    executeAllPanels,
  };
}

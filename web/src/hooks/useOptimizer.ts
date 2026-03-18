/**
 * Preact hook for connecting demo components to the WASM optimizer.
 *
 * Provides a clean interface for demos to:
 * - Toggle between simulation and WASM modes
 * - Send optimization requests
 * - Track loading/error states
 * - Configure hardware profiles and statistics
 */

import { useCallback, useEffect, useRef, useState } from "preact/hooks";

import type {
  BridgeOptimizationResult,
  OptimizerMode,
  TableStats,
  WasmStatus,
} from "src/lib/optimizer-bridge.ts";
import { getOptimizerBridge } from "src/lib/optimizer-bridge.ts";
import type {
  HardwareCategory,
  HardwareConfig,
} from "src/components/demonstrations/types.ts";

/** State returned by the useOptimizer hook. */
export interface UseOptimizerState {
  /** Current mode: "simulation" or "wasm". */
  readonly mode: OptimizerMode;
  /** Toggle between simulation and WASM mode. */
  readonly setMode: (mode: OptimizerMode) => void;
  /** WASM optimizer status. */
  readonly wasmStatus: WasmStatus;
  /** Whether an optimization is in progress. */
  readonly loading: boolean;
  /** Last error message, if any. */
  readonly error: string | null;
  /** Optimize a SQL query (WASM mode only). */
  readonly optimizeSQL: (
    sql: string,
  ) => Promise<BridgeOptimizationResult | null>;
  /** Set hardware profile for the WASM optimizer. */
  readonly setHardwareProfile: (
    category: HardwareCategory,
    config: HardwareConfig,
  ) => Promise<void>;
  /** Add table statistics for the WASM optimizer. */
  readonly addTableStats: (stats: TableStats) => Promise<void>;
  /** Whether WASM mode is available. */
  readonly wasmAvailable: boolean;
}

/** Hook for connecting demo components to the optimizer. */
export function useOptimizer(): UseOptimizerState {
  const [mode, setMode] = useState<OptimizerMode>("simulation");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [wasmStatus, setWasmStatus] = useState<WasmStatus>({
    available: false,
    loading: false,
    error: null,
    version: null,
  });
  const initAttempted = useRef(false);

  useEffect(() => {
    const bridge = getOptimizerBridge();
    const unsubscribe = bridge.onStatusChange(setWasmStatus);

    if (!initAttempted.current) {
      initAttempted.current = true;
      bridge.init().catch(() => {
        // Init failure is expected when WASM is not built.
        // Status will reflect the error via the listener.
      });
    }

    return unsubscribe;
  }, []);

  const optimizeSQL = useCallback(
    async (
      sql: string,
    ): Promise<BridgeOptimizationResult | null> => {
      if (mode !== "wasm") return null;

      const bridge = getOptimizerBridge();
      if (!bridge.status.available) {
        setError("WASM optimizer not available");
        return null;
      }

      setLoading(true);
      setError(null);
      try {
        const result = await bridge.optimizeSQL(sql);
        return result;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg);
        return null;
      } finally {
        setLoading(false);
      }
    },
    [mode],
  );

  const setHardwareProfile = useCallback(
    async (
      category: HardwareCategory,
      config: HardwareConfig,
    ): Promise<void> => {
      const bridge = getOptimizerBridge();
      if (!bridge.status.available) return;
      try {
        await bridge.setHardwareProfile(category, config);
      } catch {
        // Non-critical: profile set failure in stub mode
      }
    },
    [],
  );

  const addTableStats = useCallback(
    async (stats: TableStats): Promise<void> => {
      const bridge = getOptimizerBridge();
      if (!bridge.status.available) return;
      try {
        await bridge.addTableStats(stats);
      } catch {
        // Non-critical
      }
    },
    [],
  );

  return {
    mode,
    setMode,
    wasmStatus,
    loading,
    error,
    optimizeSQL,
    setHardwareProfile,
    addTableStats,
    wasmAvailable: wasmStatus.available,
  };
}

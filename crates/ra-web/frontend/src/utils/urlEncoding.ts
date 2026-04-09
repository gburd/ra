import type { AppState, Engine } from '../types';

interface EncodedState {
  s: string;
  e: Engine[];
  m: 'explain' | 'analyze';
}

export function encodeState(state: AppState): string {
  const encoded: EncodedState = {
    s: state.sql,
    e: state.panels.map(p => p.engine),
    m: state.explainMode,
  };

  const json = JSON.stringify(encoded);
  const base64 = btoa(json);
  return base64.replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');
}

export function decodeState(encoded: string): Partial<AppState> | null {
  try {
    const base64 = encoded.replace(/-/g, '+').replace(/_/g, '/');
    const padding = '='.repeat((4 - base64.length % 4) % 4);
    const json = atob(base64 + padding);
    const decoded = JSON.parse(json) as EncodedState;

    return {
      sql: decoded.s,
      explainMode: decoded.m,
      panels: decoded.e.map((engine, index) => ({
        id: `panel-${index}`,
        engine,
        output: null,
        rawPlan: null,
        parsedPlan: null,
        costMetrics: null,
        warnings: null,
        loading: false,
        error: null,
        activeTab: 'raw' as const,
      })),
    };
  } catch {
    return null;
  }
}

export function generateShareUrl(state: AppState): string {
  const encoded = encodeState(state);
  const baseUrl = window.location.origin;
  return `${baseUrl}/p/${encoded}`;
}

export function getStateFromUrl(): Partial<AppState> | null {
  const path = window.location.pathname;
  const match = /^\/p\/([A-Za-z0-9_-]+)$/.exec(path);

  if (!match || !match[1]) {
    return null;
  }

  return decodeState(match[1]);
}

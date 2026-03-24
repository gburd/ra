// Custom syntax highlighting for Ra documentation
// Direct imports avoid esbuild bundling issues
import rraGrammar from './rra.tmLanguage.json' assert { type: 'json' }
import algebraGrammar from './algebra.tmLanguage.json' assert { type: 'json' }
import sqlInteractiveGrammar from './sql-interactive.tmLanguage.json' assert { type: 'json' }
import cronGrammar from './cron.tmLanguage.json' assert { type: 'json' }

// Export grammars directly
export const customGrammars = [
  rraGrammar,
  algebraGrammar,
  sqlInteractiveGrammar,
  cronGrammar
]

// Create simple fallback grammars for editor/viewer languages
// These inherit from their base language (JSON, YAML, SQL, etc.)
export const fallbackGrammars = [
  { id: 'statistics-editor', base: 'json', scopeName: 'source.statistics-editor' },
  { id: 'facts-editor', base: 'yaml', scopeName: 'source.facts-editor' },
  { id: 'query-tuner', base: 'sql', scopeName: 'source.query-tuner' },
  { id: 'cost-model', base: 'json', scopeName: 'source.cost-model' },
  { id: 'statistics-viewer', base: 'json', scopeName: 'source.statistics-viewer' },
  { id: 'schema-explorer', base: 'sql', scopeName: 'source.schema-explorer' },
  { id: 'aggregation-analyzer', base: 'sql', scopeName: 'source.aggregation-analyzer' },
  { id: 'statistics-lab', base: 'json', scopeName: 'source.statistics-lab' },
  { id: 'dialect-translator', base: 'sql', scopeName: 'source.dialect-translator' },
  { id: 'feature-matrix', base: 'markdown', scopeName: 'source.feature-matrix' },
  { id: 'hardware-simulator', base: 'yaml', scopeName: 'source.hardware-simulator' },
  { id: 'window-explorer', base: 'sql', scopeName: 'source.window-explorer' },
  { id: 'optimization-trace', base: 'json', scopeName: 'source.optimization-trace' }
]

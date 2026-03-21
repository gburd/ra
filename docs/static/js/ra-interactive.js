/**
 * Ra Interactive SQL Editor Component
 *
 * Provides interactive SQL editing with WASM-based query optimization,
 * dialect translation, and formatting capabilities.
 */

// Track initialization state
let wasmModule = null;
let isInitializing = false;
let initPromise = null;

/**
 * Initialize the WASM module
 */
async function initWasm() {
    if (wasmModule) return wasmModule;
    if (isInitializing) return initPromise;

    isInitializing = true;
    initPromise = (async () => {
        try {
            const module = await import('/static/wasm/ra_wasm_docs.js');
            await module.default();
            wasmModule = module;
            console.log('Ra WASM module loaded successfully');
            return module;
        } catch (error) {
            console.error('Failed to load Ra WASM module:', error);
            // Fallback to mock implementation for demo
            wasmModule = createMockModule();
            return wasmModule;
        }
    })();

    return initPromise;
}

/**
 * Create a mock module for demo purposes when WASM is not available
 */
function createMockModule() {
    return {
        parse_sql: (sql) => {
            return JSON.stringify({
                success: true,
                expr: `RelExpr::Select { ... }`,
                original_sql: sql
            });
        },
        translate: (sql, dialect) => {
            const translations = {
                'postgresql': sql.replace(/LIMIT (\d+)/, 'LIMIT $1'),
                'mysql': sql.replace(/LIMIT (\d+) OFFSET (\d+)/, 'LIMIT $2, $1'),
                'sqlite': sql,
                'duckdb': sql
            };
            return JSON.stringify({
                success: true,
                translated_sql: translations[dialect] || sql,
                dialect: dialect
            });
        },
        optimize: (sql) => {
            return JSON.stringify({
                success: true,
                original_plan: 'Scan(table) -> Filter(condition)',
                optimized_plan: 'IndexScan(table, condition)',
                original_cost: 1000,
                optimized_cost: 50,
                applied_rules: ['Predicate Pushdown', 'Index Selection']
            });
        },
        format: (sql) => {
            const formatted = sql
                .replace(/SELECT/gi, 'SELECT')
                .replace(/FROM/gi, '\nFROM')
                .replace(/WHERE/gi, '\nWHERE')
                .replace(/GROUP BY/gi, '\nGROUP BY')
                .replace(/ORDER BY/gi, '\nORDER BY')
                .replace(/LIMIT/gi, '\nLIMIT');
            return JSON.stringify({
                success: true,
                formatted_sql: formatted
            });
        }
    };
}

/**
 * Create an interactive SQL editor component
 */
class RaInteractiveEditor {
    constructor(container, initialSql = '') {
        this.container = container;
        this.sql = initialSql;
        this.currentDialect = 'postgresql';
        this.render();
        this.attachEventListeners();
    }

    render() {
        this.container.innerHTML = `
            <div class="ra-interactive-editor">
                <div class="ra-editor-toolbar">
                    <button class="ra-btn ra-btn-copy" title="Copy SQL">
                        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
                            <path d="M4 2a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V2zm2 0v8h8V2H6zM2 6a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2v-2H8v2H2V8h2V6H2z"/>
                        </svg>
                        Copy
                    </button>
                    <button class="ra-btn ra-btn-format" title="Format SQL">
                        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
                            <path d="M2 2h12v2H2V2zm0 4h8v2H2V6zm0 4h12v2H2v-2zm0 4h8v2H2v-2z"/>
                        </svg>
                        Format
                    </button>
                    <div class="ra-dialect-selector">
                        <label>Translate to:</label>
                        <select class="ra-dialect-dropdown">
                            <option value="">Select dialect...</option>
                            <option value="postgresql">PostgreSQL</option>
                            <option value="mysql">MySQL</option>
                            <option value="sqlite">SQLite</option>
                            <option value="duckdb">DuckDB</option>
                        </select>
                    </div>
                    <button class="ra-btn ra-btn-optimize" title="Show query plan">
                        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
                            <path d="M8 1a7 7 0 1 0 0 14A7 7 0 0 0 8 1zm0 2a5 5 0 1 1 0 10A5 5 0 0 1 8 3zm0 2a1 1 0 0 0-1 1v2.586l-1.293 1.293a1 1 0 1 0 1.414 1.414l1.586-1.586A1 1 0 0 0 9 9V6a1 1 0 0 0-1-1z"/>
                        </svg>
                        Optimize
                    </button>
                </div>
                <div class="ra-editor-container">
                    <textarea class="ra-sql-editor" spellcheck="false">${this.escapeHtml(this.sql)}</textarea>
                    <div class="ra-syntax-highlight"></div>
                </div>
                <div class="ra-output-panel" style="display: none;">
                    <div class="ra-output-header">
                        <span class="ra-output-title"></span>
                        <button class="ra-close-output">×</button>
                    </div>
                    <div class="ra-output-content"></div>
                </div>
            </div>
        `;

        // Cache DOM elements
        this.elements = {
            editor: this.container.querySelector('.ra-sql-editor'),
            copyBtn: this.container.querySelector('.ra-btn-copy'),
            formatBtn: this.container.querySelector('.ra-btn-format'),
            optimizeBtn: this.container.querySelector('.ra-btn-optimize'),
            dialectSelect: this.container.querySelector('.ra-dialect-dropdown'),
            outputPanel: this.container.querySelector('.ra-output-panel'),
            outputTitle: this.container.querySelector('.ra-output-title'),
            outputContent: this.container.querySelector('.ra-output-content'),
            closeOutput: this.container.querySelector('.ra-close-output'),
            syntaxHighlight: this.container.querySelector('.ra-syntax-highlight')
        };

        // Apply syntax highlighting
        this.updateSyntaxHighlighting();
    }

    attachEventListeners() {
        // Editor input
        this.elements.editor.addEventListener('input', (e) => {
            this.sql = e.target.value;
            this.updateSyntaxHighlighting();
        });

        // Copy button
        this.elements.copyBtn.addEventListener('click', () => this.copySql());

        // Format button
        this.elements.formatBtn.addEventListener('click', () => this.formatSql());

        // Optimize button
        this.elements.optimizeBtn.addEventListener('click', () => this.optimizeSql());

        // Dialect selector
        this.elements.dialectSelect.addEventListener('change', (e) => {
            if (e.target.value) {
                this.translateSql(e.target.value);
            }
        });

        // Close output panel
        this.elements.closeOutput.addEventListener('click', () => {
            this.elements.outputPanel.style.display = 'none';
        });
    }

    async copySql() {
        try {
            await navigator.clipboard.writeText(this.sql);
            this.showToast('SQL copied to clipboard');
        } catch (err) {
            console.error('Failed to copy:', err);
            this.showToast('Failed to copy SQL', 'error');
        }
    }

    async formatSql() {
        const wasm = await initWasm();
        try {
            const result = JSON.parse(wasm.format(this.sql));
            if (result.success && result.formatted_sql) {
                this.sql = result.formatted_sql;
                this.elements.editor.value = this.sql;
                this.updateSyntaxHighlighting();
                this.showToast('SQL formatted');
            } else {
                this.showToast(result.error || 'Failed to format SQL', 'error');
            }
        } catch (err) {
            console.error('Format error:', err);
            this.showToast('Failed to format SQL', 'error');
        }
    }

    async translateSql(dialect) {
        const wasm = await initWasm();
        try {
            const result = JSON.parse(wasm.translate(this.sql, dialect));
            if (result.success && result.translated_sql) {
                this.showOutput(
                    `Translated to ${result.dialect}`,
                    this.formatCode(result.translated_sql, 'sql')
                );
            } else {
                this.showToast(result.error || 'Failed to translate SQL', 'error');
            }
        } catch (err) {
            console.error('Translation error:', err);
            this.showToast('Failed to translate SQL', 'error');
        }
    }

    async optimizeSql() {
        const wasm = await initWasm();
        try {
            const result = JSON.parse(wasm.optimize(this.sql));
            if (result.success) {
                const content = `
                    <div class="ra-optimize-result">
                        <div class="ra-plan-section">
                            <h4>Original Plan</h4>
                            <pre>${this.escapeHtml(result.original_plan || 'N/A')}</pre>
                            <div class="ra-cost">Cost: ${result.original_cost || 'N/A'}</div>
                        </div>
                        <div class="ra-plan-section">
                            <h4>Optimized Plan</h4>
                            <pre>${this.escapeHtml(result.optimized_plan || 'N/A')}</pre>
                            <div class="ra-cost">Cost: ${result.optimized_cost || 'N/A'}</div>
                        </div>
                        <div class="ra-rules-section">
                            <h4>Applied Optimizations</h4>
                            <ul>
                                ${result.applied_rules.map(rule =>
                                    `<li>${this.escapeHtml(rule)}</li>`
                                ).join('')}
                            </ul>
                        </div>
                    </div>
                `;
                this.showOutput('Query Optimization', content);
            } else {
                this.showToast(result.error || 'Failed to optimize SQL', 'error');
            }
        } catch (err) {
            console.error('Optimization error:', err);
            this.showToast('Failed to optimize SQL', 'error');
        }
    }

    updateSyntaxHighlighting() {
        // Simple SQL syntax highlighting
        const keywords = [
            'SELECT', 'FROM', 'WHERE', 'GROUP BY', 'HAVING', 'ORDER BY',
            'LIMIT', 'OFFSET', 'JOIN', 'INNER', 'LEFT', 'RIGHT', 'FULL',
            'ON', 'AND', 'OR', 'NOT', 'IN', 'EXISTS', 'BETWEEN', 'LIKE',
            'AS', 'DISTINCT', 'ALL', 'UNION', 'INTERSECT', 'EXCEPT',
            'WITH', 'RECURSIVE', 'INSERT', 'UPDATE', 'DELETE', 'CREATE',
            'DROP', 'ALTER', 'TABLE', 'INDEX', 'VIEW', 'FUNCTION'
        ];

        let highlighted = this.escapeHtml(this.sql);

        // Highlight keywords
        keywords.forEach(keyword => {
            const regex = new RegExp(`\\b${keyword}\\b`, 'gi');
            highlighted = highlighted.replace(regex,
                `<span class="ra-keyword">${keyword}</span>`);
        });

        // Highlight strings
        highlighted = highlighted.replace(/'([^']*)'/g,
            `<span class="ra-string">'$1'</span>`);

        // Highlight numbers
        highlighted = highlighted.replace(/\b(\d+)\b/g,
            `<span class="ra-number">$1</span>`);

        // Highlight comments
        highlighted = highlighted.replace(/--.*$/gm,
            `<span class="ra-comment">$&</span>`);

        this.elements.syntaxHighlight.innerHTML = highlighted;
    }

    showOutput(title, content) {
        this.elements.outputTitle.textContent = title;
        this.elements.outputContent.innerHTML = content;
        this.elements.outputPanel.style.display = 'block';
    }

    showToast(message, type = 'success') {
        const toast = document.createElement('div');
        toast.className = `ra-toast ra-toast-${type}`;
        toast.textContent = message;
        document.body.appendChild(toast);

        setTimeout(() => {
            toast.classList.add('ra-toast-show');
        }, 10);

        setTimeout(() => {
            toast.classList.remove('ra-toast-show');
            setTimeout(() => toast.remove(), 300);
        }, 3000);
    }

    formatCode(code, language) {
        return `<pre class="ra-code ra-code-${language}">${this.escapeHtml(code)}</pre>`;
    }

    escapeHtml(text) {
        const div = document.createElement('div');
        div.textContent = text;
        return div.innerHTML;
    }
}

/**
 * Initialize all interactive SQL blocks on page load
 */
document.addEventListener('DOMContentLoaded', async () => {
    // Find all sql-interactive code blocks
    const codeBlocks = document.querySelectorAll('pre code.language-sql-interactive');

    for (const block of codeBlocks) {
        const sql = block.textContent.trim();
        const container = document.createElement('div');
        container.className = 'ra-interactive-container';

        // Replace the static code block with interactive editor
        block.parentElement.replaceWith(container);

        // Create interactive editor
        new RaInteractiveEditor(container, sql);
    }

    // Pre-load WASM module
    initWasm().catch(console.error);
});

// Export for use in other modules
window.RaInteractiveEditor = RaInteractiveEditor;
window.initRaWasm = initWasm;
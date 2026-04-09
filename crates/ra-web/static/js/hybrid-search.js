// Hybrid Search Demo JavaScript

let currentEmbedding = null;

// Initialize UI
document.addEventListener('DOMContentLoaded', () => {
    // Alpha slider updates
    const alphaSlider = document.getElementById('alpha');
    const alphaValue = document.getElementById('alphaValue');
    alphaSlider.addEventListener('input', (e) => {
        alphaValue.textContent = parseFloat(e.target.value).toFixed(2);
    });

    // Generate initial embedding
    generateEmbedding();
});

// Generate embedding from query text
function generateEmbedding() {
    const query = document.getElementById('query').value;
    if (!query.trim()) {
        showError('Please enter a search query');
        return;
    }

    // Mock embedding generation (in production, this would call an embedding API)
    // Generate a 384-dimensional embedding (common for sentence transformers)
    currentEmbedding = Array.from({ length: 384 }, () => Math.random() - 0.5);

    showMessage('Embedding generated (384 dimensions)');
}

// Execute hybrid search
async function executeSearch() {
    if (!currentEmbedding) {
        generateEmbedding();
    }

    const query = document.getElementById('query').value;
    const database = document.getElementById('database').value;
    const dataset = document.getElementById('dataset').value;
    const alpha = parseFloat(document.getElementById('alpha').value);
    const limit = parseInt(document.getElementById('limit').value);

    if (!query.trim()) {
        showError('Please enter a search query');
        return;
    }

    // Build database config
    const databaseConfig = buildDatabaseConfig(database);

    // Show loading state
    document.getElementById('loading').style.display = 'block';
    document.getElementById('results').style.display = 'none';
    document.getElementById('error').style.display = 'none';

    try {
        const response = await fetch('/api/hybrid-search', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                query,
                embedding: currentEmbedding,
                database: databaseConfig,
                alpha,
                limit,
                dataset,
            }),
        });

        if (!response.ok) {
            const error = await response.json();
            throw new Error(error.message || 'Search failed');
        }

        const data = await response.json();
        displayResults(data);
    } catch (error) {
        showError(`Search error: ${error.message}`);
    } finally {
        document.getElementById('loading').style.display = 'none';
    }
}

// Build database configuration object
function buildDatabaseConfig(database) {
    switch (database) {
        case 'postgresql':
            return {
                type: 'postgresql',
                connection_string: 'postgresql://localhost/hybrid_search',
                pool_size: 5,
            };
        case 'sqlite':
            return {
                type: 'sqlite',
                database_path: './hybrid_search.db',
            };
        default:
            return {
                type: 'sqlite',
                database_path: ':memory:',
            };
    }
}

// Display search results
function displayResults(data) {
    // Show results container
    document.getElementById('results').style.display = 'block';

    // Display metrics
    displayMetrics(data.metrics);

    // Display three columns of results
    displayBM25Results(data.bm25_results);
    displayVectorResults(data.vector_results);
    displayHybridResults(data.hybrid_results);

    // Display SQL query
    document.getElementById('sqlQuery').textContent = data.sql_query;
}

// Display performance metrics
function displayMetrics(metrics) {
    document.getElementById('strategyBadge').textContent = `Strategy: ${metrics.strategy}`;
    document.getElementById('totalTime').textContent = `${metrics.total_time_ms.toFixed(2)}ms`;
    document.getElementById('bm25Time').textContent = `${metrics.bm25_time_ms.toFixed(2)}ms`;
    document.getElementById('vectorTime').textContent = `${metrics.vector_time_ms.toFixed(2)}ms`;
    document.getElementById('fusionTime').textContent = `${metrics.fusion_time_ms.toFixed(2)}ms`;
    document.getElementById('rowsScanned').textContent = metrics.rows_scanned.toLocaleString();
}

// Display BM25 results
function displayBM25Results(results) {
    const container = document.getElementById('bm25Results');
    const timeBadge = document.getElementById('bm25ResultTime');

    timeBadge.textContent = `${results.execution_time_ms.toFixed(2)}ms`;

    if (results.results.length === 0) {
        container.innerHTML = '<p style="color: #999;">No results</p>';
        return;
    }

    container.innerHTML = results.results.map(result => `
        <div class="result-item">
            <div class="result-title">${escapeHtml(result.title)}</div>
            <div class="result-snippet">${escapeHtml(result.snippet)}</div>
            <div class="result-scores">
                <span class="score score-bm25">BM25: ${result.bm25_score.toFixed(2)}</span>
            </div>
        </div>
    `).join('');
}

// Display vector results
function displayVectorResults(results) {
    const container = document.getElementById('vectorResults');
    const timeBadge = document.getElementById('vectorResultTime');

    timeBadge.textContent = `${results.execution_time_ms.toFixed(2)}ms`;

    if (results.results.length === 0) {
        container.innerHTML = '<p style="color: #999;">No results</p>';
        return;
    }

    container.innerHTML = results.results.map(result => `
        <div class="result-item">
            <div class="result-title">${escapeHtml(result.title)}</div>
            <div class="result-snippet">${escapeHtml(result.snippet)}</div>
            <div class="result-scores">
                <span class="score score-vector">Vector: ${result.vector_score.toFixed(3)}</span>
            </div>
        </div>
    `).join('');
}

// Display hybrid results
function displayHybridResults(results) {
    const container = document.getElementById('hybridResults');
    const timeBadge = document.getElementById('hybridResultTime');

    timeBadge.textContent = `${results.execution_time_ms.toFixed(2)}ms`;

    if (results.results.length === 0) {
        container.innerHTML = '<p style="color: #999;">No results</p>';
        return;
    }

    container.innerHTML = results.results.map(result => `
        <div class="result-item">
            <div class="result-title">${escapeHtml(result.title)}</div>
            <div class="result-snippet">${escapeHtml(result.snippet)}</div>
            <div class="result-scores">
                <span class="score score-bm25">BM25: ${result.bm25_score.toFixed(2)}</span>
                <span class="score score-vector">Vec: ${result.vector_score.toFixed(3)}</span>
                <span class="score score-hybrid">Hybrid: ${result.hybrid_score.toFixed(3)}</span>
            </div>
        </div>
    `).join('');
}

// Clear results
function clearResults() {
    document.getElementById('results').style.display = 'none';
    document.getElementById('error').style.display = 'none';
    document.getElementById('query').value = '';
    currentEmbedding = null;
}

// Show error message
function showError(message) {
    const errorDiv = document.getElementById('error');
    errorDiv.textContent = message;
    errorDiv.style.display = 'block';
    document.getElementById('results').style.display = 'none';
}

// Show temporary message
function showMessage(message) {
    const errorDiv = document.getElementById('error');
    errorDiv.textContent = message;
    errorDiv.style.display = 'block';
    errorDiv.style.background = '#e8f5e9';
    errorDiv.style.color = '#2e7d32';

    setTimeout(() => {
        errorDiv.style.display = 'none';
        errorDiv.style.background = '#ffebee';
        errorDiv.style.color = '#c62828';
    }, 2000);
}

// Escape HTML to prevent XSS
function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

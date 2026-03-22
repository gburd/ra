import { defineConfig } from 'vitepress'
import { katex } from '@mdit/plugin-katex'

export default defineConfig({
  title: 'RA - Relational Algebra Optimizer',
  description: '1,327+ optimization rules for database query planning',
  base: '/ra/',  // Replace with your repo name for GitHub Pages
  ignoreDeadLinks: true,  // Temporarily ignore dead links during build
  head: [
    ['link', { rel: 'icon', href: '/ra/favicon.ico' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/ra/images/favicon.svg' }],
    ['link', { rel: 'apple-touch-icon', sizes: '192x192', href: '/ra/images/logo-192.png' }],
    ['link', { rel: 'stylesheet', href: '/ra/static/css/ra-interactive.css' }],
    ['script', { type: 'module', src: '/ra/static/js/ra-interactive.js' }],
    [
      'link',
      {
        rel: 'stylesheet',
        href: 'https://cdn.jsdelivr.net/npm/katex@0.16.40/dist/katex.min.css',
        integrity: 'sha384-vKruj+a13U8yHIkAyGgK1J3ArTLzrFGBbBc0tDp4ad/EyewESeXE/Iv67Aj8gKZ0',
        crossorigin: 'anonymous'
      }
    ]
  ],
  markdown: {
    config: (md) => {
      md.use(katex)

      // Override fence renderer to prevent Vue template processing in code blocks
      // This fixes Vue parser treating SQL patterns like "AS t(col)" as HTML tags
      const defaultFence = md.renderer.rules.fence
      md.renderer.rules.fence = (tokens, idx, options, env, self) => {
        const token = tokens[idx]
        const code = token.content
        const lang = token.info.trim()

        // Escape HTML and wrap in v-pre to disable Vue template compilation
        const escapedCode = md.utils.escapeHtml(code)
        return `<pre v-pre class="language-${lang}"><code>${escapedCode}</code></pre>\n`
      }
    }
  },
  vue: {
    template: {
      compilerOptions: {
        isCustomElement: (tag) => ['t', 'r', 's', 'v', 'u', 'x', 'y', 'z', 'result', 'tbl', 'lateral', 'unnest_result'].includes(tag)
      }
    }
  },
  themeConfig: {
    logo: '/images/logo.svg',
    nav: [
      { text: 'Guide', link: '/GETTING_STARTED' },
      { text: 'Rules', link: '/rules/' },
      { text: 'API', link: '/api-reference' },
      { text: 'Architecture', link: '/architecture' }
    ],
    sidebar: {
      '/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Introduction', link: '/README' },
            { text: 'Quick Start', link: '/GETTING_STARTED' },
            { text: 'Contributing', link: '/CONTRIBUTING' }
          ]
        },
        {
          text: 'Guides',
          items: [
            { text: 'CTE Materialization', link: '/guides/cte-materialization' },
            { text: 'Development', link: '/guides/development' },
            { text: 'File Format Integration', link: '/guides/file-format-integration' },
            { text: 'Ledger Example', link: '/guides/ledger' },
            { text: 'MVs & Caching', link: '/guides/mvs-caching' },
            { text: 'Query Planner', link: '/guides/query-planner' },
            { text: 'Rule Mining', link: '/guides/rule-mining' },
            { text: 'SQL Compatibility', link: '/guides/sql-compatibility' }
          ]
        },
        {
          text: 'Concepts',
          items: [
            { text: 'Adaptive Optimization', link: '/concepts/adaptive-optimization' },
            { text: 'Algebraic Rewrites', link: '/concepts/algebraic-rewrites' },
            { text: 'Cardinality', link: '/concepts/cardinality' },
            { text: 'CBO Architecture', link: '/concepts/cbo-architecture' },
            { text: 'Distributed Query', link: '/concepts/distributed-query' },
            { text: 'Equality Saturation', link: '/concepts/equality-saturation' },
            { text: 'Federated Query', link: '/concepts/federated-query' },
            { text: 'Functional Dependencies', link: '/concepts/functional-dependencies' },
            { text: 'Physical Properties', link: '/concepts/physical-properties' },
            { text: 'Predicate Pushdown', link: '/concepts/predicate-pushdown' },
            { text: 'Row Pattern Recognition', link: '/concepts/row-pattern-recognition' },
            { text: 'Simplification Rules', link: '/concepts/simplification-rules' },
            { text: 'Statistical Estimation', link: '/concepts/statistical-estimation' }
          ]
        },
        {
          text: 'Features',
          items: [
            { text: 'Adaptive Execution', link: '/features/adaptive-execution' },
            { text: 'Distributed Query', link: '/features/distributed-query' },
            { text: 'Federated Query', link: '/features/federated-query' },
            { text: 'Index Advisor', link: '/features/index-advisor' },
            { text: 'Live Re-optimization', link: '/features/live-reoptimization' },
            { text: 'Row Pattern Recognition', link: '/features/row-pattern-recognition' },
            { text: 'Web UI', link: '/features/web-ui' }
          ]
        },
        {
          text: 'Rules',
          items: [
            { text: 'Overview', link: '/rules/' },
            { text: 'Dependency Graph', link: '/rules/dependency-graph' },
            { text: 'Aggregation', link: '/rules/aggregation' },
            { text: 'Distributed', link: '/rules/distributed' },
            { text: 'Join', link: '/rules/join' },
            { text: 'Logical', link: '/rules/logical' },
            { text: 'Physical', link: '/rules/physical' },
            { text: 'Predicate', link: '/rules/predicate' },
            { text: 'Project', link: '/rules/project' },
            { text: 'Simplification', link: '/rules/simplification' },
            { text: 'Subquery', link: '/rules/subquery' }
          ]
        },
        {
          text: 'Research',
          items: [
            { text: 'Papers', link: '/research/papers' },
            { text: 'Database Mining', link: '/research/database-mining' },
            { text: 'Rule Discovery', link: '/research/rule-discovery' }
          ]
        },
        {
          text: 'Examples',
          items: [
            { text: 'Basic Queries', link: '/examples/basic-queries' },
            { text: 'Interactive SQL Demo', link: '/examples/interactive-sql-demo' },
            { text: 'Ledger Application', link: '/examples/ledger/' },
            { text: 'Ledger Optimization Lab', link: '/examples/ledger/interactive' },
            { text: 'TPC-H Queries', link: '/examples/tpch' }
          ]
        },
        {
          text: 'Integrations',
          items: [
            { text: 'Apache Arrow', link: '/integrations/arrow' },
            { text: 'PostgreSQL', link: '/integrations/postgres' },
            { text: 'SQLite', link: '/integrations/sqlite' },
            { text: 'DataFusion', link: '/integrations/datafusion' }
          ]
        },
        {
          text: 'Reference',
          items: [
            { text: 'API Reference', link: '/api-reference' },
            { text: 'Architecture', link: '/architecture' },
            { text: 'Deployment', link: '/deployment' },
            { text: 'Testing', link: '/testing' }
          ]
        },
        {
          text: 'Maintainers',
          collapsed: true,
          items: [
            { text: 'Overview', link: '/maintainers/' },
            { text: 'Build & Install', link: '/maintainers/build' },
            { text: 'Component APIs', link: '/maintainers/components' },
            { text: 'RFCs Index', link: '/maintainers/rfcs/' },
            { text: 'Chores & Tasks', link: '/maintainers/chores' },
            { text: 'Bugs & Issues', link: '/maintainers/bugs' },
            { text: 'Release Process', link: '/maintainers/release' }
          ]
        }
      ]
    },
    socialLinks: [
      { icon: 'github', link: 'https://codeberg.org/gregburd/ra' }
    ],
    search: {
      provider: 'local'
    }
  }
})
import { defineConfig } from 'vitepress'
import { katex } from '@mdit/plugin-katex'
import { withMermaid } from 'vitepress-plugin-mermaid'
import { algebraPlugin } from './plugins/algebra-plugin.js'
import { tryItPlugin } from './plugins/try-it-plugin.js'
import algebraGrammar from './syntax/algebra.tmLanguage.json'
import rraGrammar from './syntax/rra.tmLanguage.json'
import sqlInteractiveGrammar from './syntax/sql-interactive.tmLanguage.json'
import cronGrammar from './syntax/cron.tmLanguage.json'

export default withMermaid(defineConfig({
  title: 'Ra Optimizer',
  description: '1,327+ optimization rules for database query planning',
  base: '/ra/',
  ignoreDeadLinks: false,  // Catch broken links
  head: [
    ['link', { rel: 'icon', href: '/ra/favicon.ico' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/ra/images/favicon.svg' }],
    ['link', { rel: 'apple-touch-icon', sizes: '192x192', href: '/ra/images/logo-192.png' }],
    // Temporarily disabled until WASM module is built
    // ['link', { rel: 'stylesheet', href: '/ra/static/css/ra-interactive.css' }],
    // ['script', { type: 'module', src: '/ra/static/js/ra-interactive.js' }],
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
    languages: [
      { ...algebraGrammar, name: 'algebra' },
      { ...rraGrammar, name: 'rra' },
      { ...sqlInteractiveGrammar, name: 'sql-interactive' },
      { ...cronGrammar, name: 'cron' },
    ],
    config: (md) => {
      md.use(katex)
      md.use(algebraPlugin)
      md.use(tryItPlugin)
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
      { text: 'Guide', link: '/getting-started' },
      { text: 'Rules', link: '/rules/' },
      { text: 'API', link: '/api-reference' },
      { text: 'Architecture', link: '/architecture' }
    ],
    outline: [2, 3],
    sidebar: {
      '/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Introduction', link: '/readme' },
            { text: 'Quick Start', link: '/getting-started' },
            { text: 'Architecture', link: '/architecture' },
            { text: 'Contributing', link: '/contributing' }
          ]
        },
        {
          text: 'Guides',
          items: [
            { text: 'Implementation', link: '/guides/implementation' },
            { text: 'Optimization', link: '/guides/optimization' },
            { text: 'Rule Authoring', link: '/guides/rule-authoring' },
            { text: 'Dialect Translation', link: '/guides/dialect-translation' },
            { text: 'Cost Models', link: '/guides/cost-models' },
            { text: 'Testing', link: '/guides/testing' },
            { text: 'Test Format', link: '/guides/test-format' },
            { text: 'Production Workloads', link: '/guides/modeling-production-workloads' }
          ]
        },
        {
          text: 'Concepts',
          collapsed: true,
          items: [
            { text: 'Relational Algebra', link: '/concepts/relational-algebra' },
            { text: 'Pre-Conditions', link: '/concepts/pre-conditions' },
            { text: 'Facts Provider', link: '/concepts/facts-provider' },
            { text: 'Rule Categories', link: '/concepts/rule-categories' }
          ]
        },
        {
          text: 'Features',
          collapsed: true,
          items: [
            { text: 'Adaptive Execution', link: '/features/adaptive-execution' },
            { text: 'Bitmap Index Scan', link: '/features/bitmap-index-scan' },
            { text: 'Distributed Optimization', link: '/features/distributed-optimization' },
            { text: 'Execution Models', link: '/features/execution-models' },
            { text: 'Federated Queries', link: '/features/federated-queries' },
            { text: 'Formal Verification', link: '/features/formal-verification' },
            { text: 'Function Catalog', link: '/features/function-catalog' },
            { text: 'Hardware Acceleration', link: '/features/hardware-acceleration' },
            { text: 'Index Types', link: '/features/index-types' },
            { text: 'ML Cardinality', link: '/features/ml-cardinality' },
            { text: 'Multi-Model Optimization', link: '/features/multi-model-optimization' },
            { text: 'Network Modeling', link: '/features/network-modeling' },
            { text: 'Plan Visualization', link: '/features/plan-visualization' },
            { text: 'Platform Architecture', link: '/features/platform-architecture' },
            { text: 'Resource Budgets', link: '/features/resource-budgets' },
            { text: 'SQL Coverage', link: '/features/sql-coverage' },
            { text: 'Statistics Timeline', link: '/features/statistics-timeline-format' },
            { text: 'WASM Databases', link: '/features/wasm-databases' }
          ]
        },
        {
          text: 'Rules',
          collapsed: true,
          items: [
            { text: 'Overview', link: '/rules/' },
            { text: 'Index', link: '/rules/rule-index' },
            { text: 'By Category', link: '/rules/by-category' },
            { text: 'By Database', link: '/rules/by-database' },
            { text: 'Dependency Graph', link: '/rules/dependency-graph' },
            { text: 'References', link: '/rules/references' }
          ]
        },
        {
          text: 'Examples',
          collapsed: true,
          items: [
            { text: 'Simple Optimization', link: '/examples/simple-optimization' },
            { text: 'Predicate Pushdown', link: '/examples/predicate-pushdown' },
            { text: 'Join Reordering', link: '/examples/join-reordering' },
            { text: 'Index Selection', link: '/examples/index-selection' },
            { text: 'Cost Calibration', link: '/examples/cost-calibration' },
            { text: 'Hardware-Aware', link: '/examples/hardware-aware-optimization' },
            { text: 'Distributed Joins', link: '/examples/distributed-join-strategies' },
            { text: 'Subquery Unnesting', link: '/examples/subquery-unnesting' },
            { text: 'Interactive SQL Demo', link: '/examples/interactive-sql-demo' },
            { text: 'Ledger Application', link: '/examples/ledger/' }
          ]
        },
        {
          text: 'Integrations',
          collapsed: true,
          items: [
            { text: 'Database Adapters', link: '/integrations/database-adapters' },
            { text: 'PostgreSQL', link: '/integrations/postgresql' },
            { text: 'Web UI', link: '/integrations/web-ui' }
          ]
        },
        {
          text: 'Internals',
          collapsed: true,
          items: [
            { text: 'Optimizer Architecture', link: '/internals/optimizer-architecture' },
            { text: 'E-Graph Equality Saturation', link: '/internals/egraph' },
            { text: 'Cost Model', link: '/internals/cost-model' },
            { text: 'Plan Cache', link: '/internals/plan-cache' },
            { text: 'Streaming Statistics', link: '/internals/streaming-statistics' },
            { text: 'Genetic Fingerprinting', link: '/internals/genetic-fingerprinting' },
            { text: 'Timely/Differential Dataflow', link: '/internals/timely-differential-dataflow' }
          ]
        },
        {
          text: 'Reference',
          collapsed: true,
          items: [
            { text: 'API Reference', link: '/api-reference' },
            { text: 'SQL Glossary', link: '/reference/sql-glossary' },
            { text: 'Deployment', link: '/deployment' }
          ]
        },
        {
          text: 'Maintainers',
          collapsed: true,
          items: [
            { text: 'Overview', link: '/maintainers/index' },
            { text: 'Build & Install', link: '/maintainers/build' },
            { text: 'Building Docs', link: '/building' },
            { text: 'CLI Reference', link: '/maintainers/cli-reference' },
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
  },
  mermaid: {
    // Mermaid configuration
    theme: 'default',
    securityLevel: 'loose',
    flowchart: {
      useMaxWidth: true,
      htmlLabels: true
    }
  }
}))
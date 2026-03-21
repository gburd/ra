export default {
  title: 'RA - Relational Algebra Optimizer',
  description: '1,327+ optimization rules for database query planning',
  base: '/ra/',  // Replace with your repo name for GitHub Pages
  ignoreDeadLinks: true,  // Temporarily ignore dead links during build
  themeConfig: {
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
            { text: 'Ledger Application', link: '/examples/ledger/' },
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
}
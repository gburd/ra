interface HomePageProps {
  readonly path: string;
}

export function HomePage(_props: HomePageProps) {
  return (
    <div class="home-page">
      <section class="hero">
        <h1>Relational Algebra Explorer</h1>
        <p class="subtitle">
          Compare SQL behavior across databases. Visualize query plans.
          Test isolation levels interactively.
        </p>
      </section>

      <section class="features">
        <a href="/editor" class="feature-card">
          <h2>SQL Editor</h2>
          <p>
            Write and execute SQL with syntax highlighting. See query
            plans and optimization rules applied.
          </p>
        </a>

        <a href="/compare" class="feature-card">
          <h2>Database Comparison</h2>
          <p>
            Run the same query across SQLite and DuckDB side by side.
            See how results and performance differ.
          </p>
        </a>

        <a href="/isolation" class="feature-card">
          <h2>Isolation Testing</h2>
          <p>
            Step through concurrent transactions. Observe locks,
            visibility, and anomalies in real time.
          </p>
        </a>

        <a href="/translate" class="feature-card">
          <h2>SQL Translation</h2>
          <p>
            Translate SQL between PostgreSQL, MySQL, SQLite, DuckDB,
            MSSQL, and Oracle. See syntax differences.
          </p>
        </a>

        <a href="/visualize" class="feature-card">
          <h2>Plan Visualizer</h2>
          <p>
            Compare query execution plans across Ra, PostgreSQL, MySQL,
            and DuckDB side by side with interactive tree visualization.
          </p>
        </a>

        <a href="/demos" class="feature-card">
          <h2>Interactive Demos</h2>
          <p>
            Explore how statistics staleness, hardware profiles, and
            cost model parameters affect query optimizer decisions.
          </p>
        </a>
      </section>
    </div>
  );
}

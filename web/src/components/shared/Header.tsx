import type { DatabaseId } from "src/types.ts";
import { DATABASES } from "src/types.ts";
import { route } from "preact-router";

interface HeaderProps {
  readonly activeDb: DatabaseId;
  readonly onDbChange: (db: DatabaseId) => void;
}

export function Header({ activeDb, onDbChange }: HeaderProps) {
  return (
    <header class="header">
      <div class="header-left">
        <a href="/" class="logo" onClick={() => route("/")}>
          RA
        </a>
        <nav class="nav">
          <a href="/editor" class="nav-link">
            Editor
          </a>
          <a href="/compare" class="nav-link">
            Compare
          </a>
          <a href="/isolation" class="nav-link">
            Isolation
          </a>
          <a href="/translate" class="nav-link">
            Translate
          </a>
        </nav>
      </div>
      <div class="header-right">
        <DatabaseSelector active={activeDb} onChange={onDbChange} />
      </div>
    </header>
  );
}

interface DatabaseSelectorProps {
  readonly active: DatabaseId;
  readonly onChange: (db: DatabaseId) => void;
}

function DatabaseSelector({ active, onChange }: DatabaseSelectorProps) {
  return (
    <div class="db-selector">
      {DATABASES.map((db) => (
        <button
          key={db.id}
          class={`db-btn ${active === db.id ? "active" : ""} ${
            !db.available ? "disabled" : ""
          }`}
          disabled={!db.available}
          onClick={() => onChange(db.id)}
          style={{ borderColor: db.color }}
          title={db.available ? db.name : `${db.name} (coming soon)`}
        >
          {db.name}
        </button>
      ))}
    </div>
  );
}

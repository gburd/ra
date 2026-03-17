import { useState } from "preact/hooks";

/** A column in a table schema. */
export interface SchemaColumn {
  readonly name: string;
  readonly type: string;
  readonly nullable: boolean;
  readonly primary_key: boolean;
}

/** A table schema. */
export interface TableSchema {
  readonly name: string;
  readonly columns: readonly SchemaColumn[];
}

interface SchemaViewerProps {
  readonly tables: readonly TableSchema[];
}

/**
 * Displays table schemas in a collapsible tree format.
 */
export function SchemaViewer({ tables }: SchemaViewerProps) {
  if (tables.length === 0) {
    return <p class="empty-state">No tables found.</p>;
  }

  return (
    <div class="schema-viewer">
      {tables.map((table) => (
        <SchemaTable key={table.name} table={table} />
      ))}
    </div>
  );
}

interface SchemaTableProps {
  readonly table: TableSchema;
}

function SchemaTable({ table }: SchemaTableProps) {
  const [expanded, setExpanded] = useState(true);

  return (
    <div class="schema-table">
      <div
        class="schema-table-header"
        onClick={() => setExpanded(!expanded)}
        role="button"
        tabIndex={0}
      >
        <span class="schema-expand">
          {expanded ? "\u25BC" : "\u25B6"}
        </span>
        <span class="schema-table-name">{table.name}</span>
        <span class="schema-col-count">
          {table.columns.length} column
          {table.columns.length !== 1 ? "s" : ""}
        </span>
      </div>

      {expanded && (
        <div class="schema-columns">
          {table.columns.map((col) => (
            <div key={col.name} class="schema-column">
              {col.primary_key && (
                <span class="schema-pk" title="Primary Key">
                  PK
                </span>
              )}
              <span class="schema-col-name">{col.name}</span>
              <span class="schema-col-type">{col.type}</span>
              {col.nullable && (
                <span class="schema-nullable">NULL</span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

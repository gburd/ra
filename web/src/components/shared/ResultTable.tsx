import type { ResultColumn, ResultRow } from "src/types.ts";

interface ResultTableProps {
  readonly columns: readonly ResultColumn[];
  readonly rows: readonly ResultRow[];
  readonly elapsed_ms: number;
}

export function ResultTable({ columns, rows, elapsed_ms }: ResultTableProps) {
  if (columns.length === 0) {
    return (
      <div class="result-empty">
        <p>Query executed successfully ({elapsed_ms.toFixed(1)} ms).</p>
        <p>No rows returned.</p>
      </div>
    );
  }

  return (
    <div class="result-table-wrapper">
      <div class="result-status">
        {rows.length} row{rows.length !== 1 ? "s" : ""} in{" "}
        {elapsed_ms.toFixed(1)} ms
      </div>
      <div class="result-table-scroll">
        <table class="result-table">
          <thead>
            <tr>
              {columns.map((col) => (
                <th key={col.name} title={col.type}>
                  {col.name}
                  <span class="col-type">{col.type}</span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, rowIdx) => (
              <tr key={rowIdx}>
                {columns.map((col) => (
                  <td key={col.name}>
                    <CellValue value={row[col.name]} />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

interface CellValueProps {
  readonly value: unknown;
}

function CellValue({ value }: CellValueProps) {
  if (value === null || value === undefined) {
    return <span class="null-value">NULL</span>;
  }
  if (typeof value === "boolean") {
    return <span class="bool-value">{value ? "true" : "false"}</span>;
  }
  if (typeof value === "number") {
    return <span class="number-value">{String(value)}</span>;
  }
  return <span>{String(value)}</span>;
}

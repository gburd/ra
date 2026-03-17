import { useState, useCallback } from "preact/hooks";
import type {
  DatabaseId,
  IsolationLevel,
  SessionStep,
  LockInfo,
  ExecuteResponse,
} from "src/types.ts";
import { executeSQL } from "src/api/client.ts";
import { SQLEditor } from "src/components/editor/SQLEditor.tsx";
import { ResultTable } from "src/components/shared/ResultTable.tsx";

interface IsolationPageProps {
  readonly path: string;
  readonly database: DatabaseId;
}

const ISOLATION_LEVELS: readonly IsolationLevel[] = [
  "read_uncommitted",
  "read_committed",
  "repeatable_read",
  "serializable",
  "snapshot",
];

function formatIsolationLevel(level: IsolationLevel): string {
  return level
    .split("_")
    .map((w) => w[0]?.toUpperCase() + w.slice(1))
    .join(" ");
}

interface SessionState {
  readonly id: number;
  readonly isolation: IsolationLevel;
  readonly steps: readonly StepState[];
  readonly currentSql: string;
}

interface StepState {
  readonly step: SessionStep;
  readonly executed: boolean;
  readonly result?: ExecuteResponse;
  readonly error?: string;
}

export function IsolationPage({ database }: IsolationPageProps) {
  const [isolation, setIsolation] =
    useState<IsolationLevel>("read_committed");
  const [setupSql, setSetupSql] = useState(
    "CREATE TABLE accounts (id INTEGER PRIMARY KEY, balance INTEGER);\n" +
      "INSERT INTO accounts VALUES (1, 1000), (2, 2000);",
  );
  const [sessions, setSessions] = useState<SessionState[]>([
    { id: 1, isolation: "read_committed", steps: [], currentSql: "" },
    { id: 2, isolation: "read_committed", steps: [], currentSql: "" },
  ]);
  const [setupDone, setSetupDone] = useState(false);
  const [setupError, setSetupError] = useState<string | null>(null);
  const [timeline, setTimeline] = useState<readonly SessionStep[]>([]);
  const [locks, setLocks] = useState<readonly LockInfo[]>([]);
  const [playing, setPlaying] = useState(false);
  const [stepIndex, setStepIndex] = useState(0);

  const handleSetup = useCallback(async () => {
    setSetupError(null);
    try {
      await executeSQL({ sql: setupSql, database });
      setSetupDone(true);
      setSessions((prev) =>
        prev.map((s) => ({ ...s, isolation, steps: [] })),
      );
      setTimeline([]);
      setLocks([]);
      setStepIndex(0);
    } catch (err) {
      setSetupError(
        err instanceof Error ? err.message : String(err),
      );
    }
  }, [setupSql, database, isolation]);

  const handleReset = useCallback(() => {
    setSetupDone(false);
    setSetupError(null);
    setSessions((prev) =>
      prev.map((s) => ({
        ...s,
        steps: [],
        currentSql: "",
      })),
    );
    setTimeline([]);
    setLocks([]);
    setStepIndex(0);
    setPlaying(false);
  }, []);

  const addSession = useCallback(() => {
    setSessions((prev) => {
      const nextId =
        prev.reduce((max, s) => Math.max(max, s.id), 0) + 1;
      return [
        ...prev,
        {
          id: nextId,
          isolation,
          steps: [],
          currentSql: "",
        },
      ];
    });
  }, [isolation]);

  const removeSession = useCallback((sessionId: number) => {
    setSessions((prev) => prev.filter((s) => s.id !== sessionId));
  }, []);

  const updateSessionSql = useCallback(
    (sessionId: number, sql: string) => {
      setSessions((prev) =>
        prev.map((s) =>
          s.id === sessionId ? { ...s, currentSql: sql } : s,
        ),
      );
    },
    [],
  );

  const executeStep = useCallback(
    async (sessionId: number) => {
      const session = sessions.find((s) => s.id === sessionId);
      if (session === undefined || session.currentSql.trim() === "") {
        return;
      }

      const stepNumber = session.steps.length + 1;
      const newStep: SessionStep = {
        session_id: sessionId,
        step_number: stepNumber,
        sql: session.currentSql,
        locks_held: [],
      };

      try {
        const result = await executeSQL({
          sql: session.currentSql,
          database,
        });

        const completedStep: SessionStep = {
          ...newStep,
          result,
          locks_held: mockLocksForStep(
            sessionId,
            session.currentSql,
          ),
        };

        const stepState: StepState = {
          step: completedStep,
          executed: true,
          result,
        };

        setSessions((prev) =>
          prev.map((s) =>
            s.id === sessionId
              ? {
                  ...s,
                  steps: [...s.steps, stepState],
                  currentSql: "",
                }
              : s,
          ),
        );

        setTimeline((prev) => [...prev, completedStep]);
        setLocks(collectLocks(sessions, completedStep));
        setStepIndex((prev) => prev + 1);
      } catch (err) {
        const errorMsg =
          err instanceof Error ? err.message : String(err);
        const errorStep: StepState = {
          step: newStep,
          executed: true,
          error: errorMsg,
        };

        setSessions((prev) =>
          prev.map((s) =>
            s.id === sessionId
              ? {
                  ...s,
                  steps: [...s.steps, errorStep],
                  currentSql: "",
                }
              : s,
          ),
        );
      }
    },
    [sessions, database],
  );

  const handlePlayPause = useCallback(() => {
    setPlaying((prev) => !prev);
  }, []);

  const handleStepForward = useCallback(() => {
    setStepIndex((prev) =>
      Math.min(prev + 1, timeline.length),
    );
  }, [timeline.length]);

  const handleStepBack = useCallback(() => {
    setStepIndex((prev) => Math.max(prev - 1, 0));
  }, []);

  return (
    <div class="isolation-page">
      <div class="isolation-setup">
        <h2>Isolation Test Studio</h2>

        <div class="isolation-config">
          <label class="config-label">
            Isolation Level:
            <select
              class="config-select"
              value={isolation}
              onChange={(e) =>
                setIsolation(
                  (e.target as HTMLSelectElement)
                    .value as IsolationLevel,
                )
              }
            >
              {ISOLATION_LEVELS.map((level) => (
                <option key={level} value={level}>
                  {formatIsolationLevel(level)}
                </option>
              ))}
            </select>
          </label>
        </div>

        {!setupDone && (
          <div class="setup-section">
            <h3>Setup SQL</h3>
            <SQLEditor value={setupSql} onChange={setSetupSql} />
            <button
              class="btn btn-primary"
              onClick={handleSetup}
            >
              Run Setup
            </button>
            {setupError !== null && (
              <div class="error-banner">{setupError}</div>
            )}
          </div>
        )}

        {setupDone && (
          <div class="setup-done">
            <span>Setup complete.</span>
            <button class="btn btn-secondary" onClick={handleReset}>
              Reset
            </button>
            <button class="btn btn-secondary" onClick={addSession}>
              Add Session
            </button>
          </div>
        )}
      </div>

      {setupDone && (
        <>
          <div class="session-panels">
            {sessions.map((session) => (
              <SessionPanel
                key={session.id}
                session={session}
                onExecute={executeStep}
                onSqlChange={updateSessionSql}
                onRemove={
                  sessions.length > 1
                    ? removeSession
                    : undefined
                }
              />
            ))}
          </div>

          <StepController
            stepIndex={stepIndex}
            totalSteps={timeline.length}
            playing={playing}
            onPlayPause={handlePlayPause}
            onStepForward={handleStepForward}
            onStepBack={handleStepBack}
          />

          <Timeline steps={timeline} currentStep={stepIndex} />

          <LockTable locks={locks} />
        </>
      )}
    </div>
  );
}

interface SessionPanelProps {
  readonly session: SessionState;
  readonly onExecute: (sessionId: number) => void;
  readonly onSqlChange: (sessionId: number, sql: string) => void;
  readonly onRemove: ((sessionId: number) => void) | undefined;
}

function SessionPanel({
  session,
  onExecute,
  onSqlChange,
  onRemove,
}: SessionPanelProps) {
  const sessionColor = sessionColors[session.id % sessionColors.length];

  return (
    <div
      class="session-panel"
      style={{ borderTopColor: sessionColor }}
    >
      <div class="session-header">
        <span
          class="session-name"
          style={{ color: sessionColor }}
        >
          Session {session.id}
        </span>
        <span class="session-isolation">
          {formatIsolationLevel(session.isolation)}
        </span>
        {onRemove !== undefined && (
          <button
            class="btn-icon"
            onClick={() => onRemove(session.id)}
            title="Remove session"
          >
            x
          </button>
        )}
      </div>

      <div class="session-steps">
        {session.steps.map((s, i) => (
          <div key={i} class="step-entry">
            <div class="step-sql">
              <span class="step-number">#{s.step.step_number}</span>
              <code>{s.step.sql}</code>
            </div>
            {s.error !== undefined && (
              <div class="error-banner">{s.error}</div>
            )}
            {s.result !== undefined && (
              <ResultTable
                columns={s.result.columns}
                rows={s.result.rows}
                elapsed_ms={s.result.elapsed_ms}
              />
            )}
          </div>
        ))}
      </div>

      <div class="session-input">
        <textarea
          class="session-sql-input"
          value={session.currentSql}
          onInput={(e) =>
            onSqlChange(
              session.id,
              (e.target as HTMLTextAreaElement).value,
            )
          }
          placeholder="-- Enter SQL for this session..."
          rows={3}
        />
        <button
          class="btn btn-primary btn-sm"
          onClick={() => onExecute(session.id)}
          disabled={session.currentSql.trim() === ""}
        >
          Execute
        </button>
      </div>
    </div>
  );
}

interface StepControllerProps {
  readonly stepIndex: number;
  readonly totalSteps: number;
  readonly playing: boolean;
  readonly onPlayPause: () => void;
  readonly onStepForward: () => void;
  readonly onStepBack: () => void;
}

function StepController({
  stepIndex,
  totalSteps,
  playing,
  onPlayPause,
  onStepForward,
  onStepBack,
}: StepControllerProps) {
  return (
    <div class="step-controller">
      <button
        class="btn btn-sm"
        onClick={onStepBack}
        disabled={stepIndex === 0}
        title="Step back"
      >
        &lt;
      </button>
      <button
        class="btn btn-sm"
        onClick={onPlayPause}
        title={playing ? "Pause" : "Play"}
      >
        {playing ? "||" : ">"}
      </button>
      <button
        class="btn btn-sm"
        onClick={onStepForward}
        disabled={stepIndex >= totalSteps}
        title="Step forward"
      >
        &gt;
      </button>
      <span class="step-counter">
        Step {stepIndex} / {totalSteps}
      </span>
    </div>
  );
}

interface TimelineProps {
  readonly steps: readonly SessionStep[];
  readonly currentStep: number;
}

function Timeline({ steps, currentStep }: TimelineProps) {
  if (steps.length === 0) {
    return null;
  }

  return (
    <div class="timeline">
      <h3>Execution Timeline</h3>
      <div class="timeline-track">
        {steps.map((step, i) => {
          const color =
            sessionColors[step.session_id % sessionColors.length];
          const active = i < currentStep;
          return (
            <div
              key={i}
              class={`timeline-event ${active ? "active" : "future"}`}
              style={{ borderLeftColor: color }}
            >
              <span
                class="timeline-session"
                style={{ color }}
              >
                S{step.session_id}
              </span>
              <code class="timeline-sql">{step.sql}</code>
              {step.error !== undefined && (
                <span class="timeline-error">Error</span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

interface LockTableProps {
  readonly locks: readonly LockInfo[];
}

function LockTable({ locks }: LockTableProps) {
  if (locks.length === 0) {
    return null;
  }

  return (
    <div class="lock-table-section">
      <h3>Lock State</h3>
      <table class="lock-table">
        <thead>
          <tr>
            <th>Session</th>
            <th>Type</th>
            <th>Resource</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {locks.map((lock, i) => (
            <tr
              key={i}
              class={lock.granted ? "lock-granted" : "lock-waiting"}
            >
              <td>
                <span
                  style={{
                    color:
                      sessionColors[
                        lock.session_id % sessionColors.length
                      ],
                  }}
                >
                  S{lock.session_id}
                </span>
              </td>
              <td>{lock.lock_type}</td>
              <td>{lock.resource}</td>
              <td>{lock.granted ? "Granted" : "Waiting"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const sessionColors = [
  "#4A9EFF",
  "#FF6B6B",
  "#50C878",
  "#FFB347",
  "#9B59B6",
  "#1ABC9C",
] as const;

function mockLocksForStep(
  sessionId: number,
  sql: string,
): readonly LockInfo[] {
  const lower = sql.toLowerCase();
  if (lower.includes("update") || lower.includes("delete")) {
    return [
      {
        session_id: sessionId,
        lock_type: "exclusive",
        resource: "accounts",
        granted: true,
      },
    ];
  }
  if (lower.includes("select")) {
    return [
      {
        session_id: sessionId,
        lock_type: "shared",
        resource: "accounts",
        granted: true,
      },
    ];
  }
  return [];
}

function collectLocks(
  sessions: readonly SessionState[],
  _latestStep: SessionStep,
): readonly LockInfo[] {
  const allLocks: LockInfo[] = [];
  for (const session of sessions) {
    for (const stepState of session.steps) {
      for (const lock of stepState.step.locks_held) {
        allLocks.push(lock);
      }
    }
  }
  return allLocks;
}

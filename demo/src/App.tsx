import React from "react";
import { AlertCircle, CheckCircle2, Code2, Database, LoaderCircle, Play, Table2 } from "lucide-react";

import { PdlEditor } from "./PdlEditor";
import { loadPdlRuntime, type PdlEditorDiagnostic, type PdlRunResult, type PdlRuntime } from "./pdlWasm";

const DEFAULT_SOURCE = `let customers =
  load "customers.csv"
  | select "customer_id", "segment"

load "sales.csv"
  | filter "status" == "completed"
  | join customers on "customer_id" kind left
  | group_by "segment"
  | agg sum("amount") as "revenue", count() as "orders"
  | sort "revenue" desc`;

const DEFAULT_FILES: Record<string, string> = {
  "sales.csv": `sale_id,customer_id,status,amount
S1,C001,completed,120
S2,C002,pending,75
S3,C001,completed,80
S4,C003,completed,200
S5,C004,completed,50
S6,C003,completed,150
S7,C005,completed,90
`,
  "customers.csv": `customer_id,segment
C001,Enterprise
C002,SMB
C003,Enterprise
C004,Consumer
C005,SMB
`,
};

type LoadState = "loading" | "ready" | "error";

export function App(): React.ReactElement {
  const [runtime, setRuntime] = React.useState<PdlRuntime | null>(null);
  const [runtimeState, setRuntimeState] = React.useState<LoadState>("loading");
  const [runtimeError, setRuntimeError] = React.useState<string | null>(null);
  const [source, setSource] = React.useState(DEFAULT_SOURCE);
  const [files, setFiles] = React.useState<Record<string, string>>(DEFAULT_FILES);
  const [activeFile, setActiveFile] = React.useState("sales.csv");
  const [diagnostics, setDiagnostics] = React.useState<PdlEditorDiagnostic[]>([]);
  const [runResult, setRunResult] = React.useState<PdlRunResult | null>(null);
  const [running, setRunning] = React.useState(false);

  const inputStats = React.useMemo(() => csvStats(files[activeFile] ?? ""), [activeFile, files]);
  const outputStats = React.useMemo(() => csvStats(runResult?.stdout ?? ""), [runResult?.stdout]);
  const fileNames = React.useMemo(() => Object.keys(files), [files]);

  React.useEffect(() => {
    let cancelled = false;
    loadPdlRuntime()
      .then((loaded) => {
        if (cancelled) return;
        setRuntime(loaded);
        setRuntimeState("ready");
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setRuntimeError(errorMessage(error));
        setRuntimeState("error");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const runCurrent = React.useCallback(() => {
    if (!runtime) {
      return;
    }
    setRunning(true);
    window.setTimeout(() => {
      try {
        const editorResponse = runtime.editorService(source, files, { kind: "diagnostics" });
        setDiagnostics(editorResponse.diagnostics);
        setRunResult(runtime.run(source, files, "csv"));
      } catch (error: unknown) {
        setRunResult({
          stdout: null,
          diagnostics: [],
          error: errorMessage(error),
        });
      } finally {
        setRunning(false);
      }
    }, 0);
  }, [files, runtime, source]);

  React.useEffect(() => {
    if (!runtime) {
      return;
    }
    const timer = window.setTimeout(runCurrent, 220);
    return () => window.clearTimeout(timer);
  }, [runCurrent, runtime]);

  const combinedError = runResult?.error ?? runtimeError;
  const runtimeDiagnostics = runResult?.diagnostics ?? [];
  const hasDiagnostics = diagnostics.length > 0 || runtimeDiagnostics.length > 0 || Boolean(combinedError);
  const output = runResult?.stdout ?? "";
  const activeFileText = files[activeFile] ?? "";

  return (
    <div className="app-shell">
      <header className="app-header">
        <div className="brand-lockup">
          <span className="brand-mark">PDL</span>
          <span>
            <strong>WASM demo</strong>
            <small>In-memory table pipeline</small>
          </span>
        </div>
        <div className={`status-pill status-${runtimeState}`}>
          {runtimeState === "loading" ? <LoaderCircle className="spin" size={15} aria-hidden="true" /> : null}
          {runtimeState === "ready" ? <CheckCircle2 size={15} aria-hidden="true" /> : null}
          {runtimeState === "error" ? <AlertCircle size={15} aria-hidden="true" /> : null}
          {runtimeState}
        </div>
      </header>

      <main className="workspace-grid">
        <section className="pane editor-pane">
          <PaneHeader
            icon={<Code2 size={17} aria-hidden="true" />}
            title="Pipeline"
            detail={`${source.length} bytes`}
            action={
              <button className="icon-button" type="button" onClick={runCurrent} aria-label="Run pipeline">
                {running ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <Play size={16} aria-hidden="true" />}
              </button>
            }
          />
          <PdlEditor diagnostics={diagnostics} files={files} onChange={setSource} runtime={runtime} value={source} />
        </section>

        <section className="pane data-pane">
          <PaneHeader
            icon={<Database size={17} aria-hidden="true" />}
            title={activeFile}
            detail={inputStats}
          />
          <div className="file-tabs" role="tablist" aria-label="Host CSV files">
            {fileNames.map((name) => (
              <button
                aria-selected={name === activeFile}
                className={`file-tab ${name === activeFile ? "file-tab-active" : ""}`}
                key={name}
                onClick={() => setActiveFile(name)}
                role="tab"
                type="button"
              >
                {name}
              </button>
            ))}
          </div>
          <textarea
            aria-label="CSV input"
            className="csv-textarea"
            spellCheck={false}
            value={activeFileText}
            onChange={(event) =>
              setFiles((current) => ({
                ...current,
                [activeFile]: event.target.value,
              }))
            }
          />
        </section>

        <section className="pane output-pane">
          <PaneHeader
            icon={running ? <LoaderCircle className="spin" size={17} aria-hidden="true" /> : <Table2 size={17} aria-hidden="true" />}
            title="CSV output"
            detail={output ? outputStats : "No rows"}
          />
          <pre className={`csv-output ${output ? "" : "csv-output-empty"}`}>
            {output || combinedError || "Awaiting output"}
          </pre>
        </section>

        <section className={`diagnostics-pane ${hasDiagnostics ? "" : "diagnostics-ok"}`}>
          <DiagnosticsPanel
            editorDiagnostics={diagnostics}
            runtimeDiagnostics={runtimeDiagnostics}
            error={combinedError}
          />
        </section>
      </main>
    </div>
  );
}

function PaneHeader({
  icon,
  title,
  detail,
  action,
}: {
  icon: React.ReactNode;
  title: string;
  detail?: string;
  action?: React.ReactNode;
}): React.ReactElement {
  return (
    <div className="pane-header">
      <div className="pane-title">
        {icon}
        <span>{title}</span>
      </div>
      <span className="pane-detail">{detail}</span>
      {action}
    </div>
  );
}

function DiagnosticsPanel({
  editorDiagnostics,
  runtimeDiagnostics,
  error,
}: {
  editorDiagnostics: PdlEditorDiagnostic[];
  runtimeDiagnostics: PdlRunResult["diagnostics"];
  error: string | null;
}): React.ReactElement {
  if (!error && editorDiagnostics.length === 0 && runtimeDiagnostics.length === 0) {
    return (
      <div className="diagnostics-line diagnostics-line-ok">
        <CheckCircle2 size={16} aria-hidden="true" />
        No diagnostics
      </div>
    );
  }

  return (
    <div className="diagnostics-list">
      {error ? (
        <div className="diagnostic-row diagnostic-error">
          <AlertCircle size={16} aria-hidden="true" />
          <span className="diagnostic-code">Runtime</span>
          <span>{error}</span>
        </div>
      ) : null}
      {editorDiagnostics.map((diagnostic, index) => (
        <div className={`diagnostic-row diagnostic-${diagnostic.severity}`} key={`editor-${diagnostic.code}-${index}`}>
          <AlertCircle size={16} aria-hidden="true" />
          <span className="diagnostic-code">{diagnostic.code}</span>
          <span>
            {diagnostic.message}{" "}
            <span className="diagnostic-span">
              {diagnostic.range.start.line + 1}:{diagnostic.range.start.character + 1}
            </span>
          </span>
        </div>
      ))}
      {runtimeDiagnostics.map((diagnostic, index) => (
        <div className={`diagnostic-row diagnostic-${diagnostic.severity}`} key={`runtime-${diagnostic.code}-${index}`}>
          <AlertCircle size={16} aria-hidden="true" />
          <span className="diagnostic-code">{diagnostic.code}</span>
          <span>{diagnostic.message}</span>
        </div>
      ))}
    </div>
  );
}

function csvStats(csv: string): string {
  const lines = csv.split(/\r?\n/).filter((line) => line.length > 0);
  if (lines.length === 0) {
    return "0 rows";
  }
  const columns = lines[0]?.split(",").length ?? 0;
  return `${Math.max(0, lines.length - 1)} rows, ${columns} cols`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

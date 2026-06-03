import React from "react";
import { AlertCircle, CheckCircle2, Code2, LoaderCircle, Play, Table2 } from "lucide-react";

import { PdlEditor } from "../../PdlEditor";
import type { PdlEditorDiagnostic, PdlRunResult } from "../../pdlWasm";
import { type RuntimeState, usePdlRuntime } from "./usePdlRuntime";

export interface LiveExampleProps {
  id: string;
  source: string;
  files: Record<string, string>;
  stdoutFormat?: "csv" | "jsonl";
}

interface RunSnapshot {
  source: string;
  filesSignature: string;
  stdoutFormat: string;
  result: PdlRunResult;
  editorDiagnostics: PdlEditorDiagnostic[];
}

export function LiveExample({
  id,
  source,
  files,
  stdoutFormat = "csv",
}: LiveExampleProps): React.ReactElement {
  const { runtime, state: runtimeState, error: runtimeError } = usePdlRuntime();
  const [value, setValue] = React.useState(source);
  const [snapshot, setSnapshot] = React.useState<RunSnapshot | null>(null);
  const [running, setRunning] = React.useState(false);
  const filesSignature = React.useMemo(() => stableFilesSignature(files), [files]);

  const runCurrent = React.useCallback(() => {
    if (!runtime) {
      return;
    }
    const runSource = value;
    const runFiles = files;
    const runFilesSignature = filesSignature;
    setRunning(true);
    window.setTimeout(() => {
      try {
        const editorResponse = runtime.editorService(runSource, runFiles, { kind: "diagnostics" });
        setSnapshot({
          source: runSource,
          filesSignature: runFilesSignature,
          stdoutFormat,
          result: runtime.run(runSource, runFiles, stdoutFormat),
          editorDiagnostics: editorResponse.diagnostics,
        });
      } catch (error: unknown) {
        setSnapshot({
          source: runSource,
          filesSignature: runFilesSignature,
          stdoutFormat,
          result: { stdout: null, diagnostics: [], error: errorMessage(error) },
          editorDiagnostics: [],
        });
      } finally {
        setRunning(false);
      }
    }, 0);
  }, [files, filesSignature, runtime, stdoutFormat, value]);

  React.useEffect(() => {
    if (runtimeState !== "ready") {
      return;
    }
    const timer = window.setTimeout(runCurrent, 220);
    return () => window.clearTimeout(timer);
  }, [runCurrent, runtimeState]);

  const current = Boolean(
    snapshot &&
      snapshot.source === value &&
      snapshot.filesSignature === filesSignature &&
      snapshot.stdoutFormat === stdoutFormat,
  );
  const result = current ? snapshot?.result ?? null : null;
  const editorDiagnostics = current ? snapshot?.editorDiagnostics ?? [] : [];
  const runtimeDiagnostics = result?.diagnostics ?? [];
  const output = result?.stdout ?? "";
  const outputDetail = output ? outputStats(output, stdoutFormat) : "Awaiting output";
  const hasError =
    Boolean(result?.error) ||
    runtimeDiagnostics.some((diagnostic) => diagnostic.severity === "error") ||
    editorDiagnostics.some((diagnostic) => diagnostic.severity === "error");

  return (
    <div className="tutorial-live-pair">
      <section className="tutorial-example-panel tutorial-example-editor-panel">
        <div className="mini-panel-header">
          <span>
            <Code2 size={16} aria-hidden="true" />
            {id}.pdl
          </span>
          <button className="compact-button" type="button" disabled={!runtime} onClick={runCurrent}>
            {running ? <LoaderCircle className="spin" size={15} aria-hidden="true" /> : <Play size={15} aria-hidden="true" />}
            Run
          </button>
        </div>
        <div className="tutorial-example-editor">
          <PdlEditor
            diagnostics={editorDiagnostics}
            files={files}
            modelUri={`inmemory://pdl/docs/${id}.pdl`}
            onChange={setValue}
            runtime={runtime}
            value={value}
          />
        </div>
      </section>

      <section className="tutorial-example-panel tutorial-example-output-panel">
        <div className="mini-panel-header">
          <span>
            {running ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <Table2 size={16} aria-hidden="true" />}
            {stdoutFormat.toUpperCase()} stdout
          </span>
          <DocsStatus state={runtimeState} />
        </div>
        <pre className={`tutorial-output-stage ${output ? "" : "tutorial-output-empty"}`}>
          {output || result?.error || runtimeError || "Loading the browser runtime"}
        </pre>
        <DiagnosticsStrip
          detail={current ? outputDetail : "Diagnostics updating"}
          error={result?.error ?? null}
          hasError={hasError}
          runtimeDiagnostics={runtimeDiagnostics}
          stale={Boolean(snapshot && !current)}
        />
      </section>
    </div>
  );
}

function DocsStatus({ state }: { state: RuntimeState }): React.ReactElement {
  const label = state === "ready" ? "ready" : state === "loading" ? "loading" : "error";
  return <span className={`mini-status mini-status-${state}`}>WASM {label}</span>;
}

function DiagnosticsStrip({
  detail,
  error,
  hasError,
  runtimeDiagnostics,
  stale,
}: {
  detail: string;
  error: string | null;
  hasError: boolean;
  runtimeDiagnostics: PdlRunResult["diagnostics"];
  stale: boolean;
}): React.ReactElement {
  if (stale) {
    return (
      <div className="tutorial-diagnostics">
        <LoaderCircle className="spin" size={15} aria-hidden="true" />
        {detail}
      </div>
    );
  }

  if (!error && runtimeDiagnostics.length === 0) {
    return (
      <div className="tutorial-diagnostics">
        <CheckCircle2 size={15} aria-hidden="true" />
        {detail}
      </div>
    );
  }

  return (
    <div className={`tutorial-diagnostics ${hasError ? "tutorial-diagnostics-error" : ""}`}>
      <AlertCircle size={15} aria-hidden="true" />
      {error ?? `${runtimeDiagnostics.length} runtime diagnostic${runtimeDiagnostics.length === 1 ? "" : "s"}`}
    </div>
  );
}

function outputStats(text: string, format: string): string {
  const lines = text.split(/\r?\n/).filter((line) => line.length > 0);
  if (format === "jsonl") {
    return `${lines.length} JSON Lines row${lines.length === 1 ? "" : "s"}`;
  }
  if (lines.length === 0) {
    return "0 rows";
  }
  const columns = lines[0]?.split(",").length ?? 0;
  return `${Math.max(0, lines.length - 1)} rows, ${columns} cols`;
}

function stableFilesSignature(files: Record<string, string>): string {
  return JSON.stringify(Object.fromEntries(Object.entries(files).sort(([left], [right]) => left.localeCompare(right))));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

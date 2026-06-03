import React from "react";
import { ArrowRight, CheckCircle2, Code2, LoaderCircle, Play, Table2 } from "lucide-react";

import { PdlEditor } from "../PdlEditor";
import type { PdlEditorDiagnostic, PdlRunResult } from "../pdlWasm";
import { type RuntimeState, usePdlRuntime } from "./docs/usePdlRuntime";

interface RoutedPageProps {
  navigate: (path: string, event?: React.MouseEvent<HTMLAnchorElement>) => void;
  routeHref: (path: string) => string;
}

const HOMEPAGE_DATA_PATH = "sales.csv";

const STARTER_DATA = `region,status,amount,customer_age,customer_id
North,completed,120,34,C001
South,pending,75,41,C002
North,completed,80,29,C001
West,completed,200,37,C003
South,completed,50,45,C004
West,completed,150,31,C003
East,completed,90,28,C005
`;

const STARTER_SOURCE = `load "sales.csv"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", count() as "orders"
  | sort "total_revenue" desc
`;

const BUILD_COMMANDS = `cargo build -p pdl-cli
cargo run -p pdl-cli -- run examples/top_regions.pdl --stdout-format csv
`;

const INTEROP_COMMANDS = `pdl run prep.pdl --stdout-format arrow-stream > prepared.arrow
pdl run passthrough.pdl --stdin-format arrow-stream < prepared.arrow > sorted.arrow
`;

export function HomePage({ navigate, routeHref }: RoutedPageProps): React.ReactElement {
  const { runtime, state: runtimeState, error: runtimeError } = usePdlRuntime();
  const [source, setSource] = React.useState(STARTER_SOURCE);
  const [result, setResult] = React.useState<PdlRunResult | null>(null);
  const [diagnostics, setDiagnostics] = React.useState<PdlEditorDiagnostic[]>([]);
  const [running, setRunning] = React.useState(false);
  const files = React.useMemo(() => ({ [HOMEPAGE_DATA_PATH]: STARTER_DATA }), []);

  const runCurrent = React.useCallback(() => {
    if (!runtime) {
      return;
    }
    const runSource = source;
    setRunning(true);
    window.setTimeout(() => {
      try {
        const editorResponse = runtime.editorService(runSource, files, { kind: "diagnostics" });
        setDiagnostics(editorResponse.diagnostics);
        setResult(runtime.run(runSource, files, "csv"));
      } catch (error: unknown) {
        setResult({ stdout: null, diagnostics: [], error: errorMessage(error) });
      } finally {
        setRunning(false);
      }
    }, 0);
  }, [files, runtime, source]);

  React.useEffect(() => {
    if (runtimeState !== "ready") {
      return;
    }
    const timer = window.setTimeout(runCurrent, 220);
    return () => window.clearTimeout(timer);
  }, [runCurrent, runtimeState]);

  const output = result?.stdout ?? "";
  const runtimeDiagnostics = result?.diagnostics ?? [];
  const errorCount =
    diagnostics.filter((diagnostic) => diagnostic.severity === "error").length +
    runtimeDiagnostics.filter((diagnostic) => diagnostic.severity === "error").length +
    (result?.error ? 1 : 0);
  const warningCount =
    diagnostics.filter((diagnostic) => diagnostic.severity === "warning").length +
    runtimeDiagnostics.filter((diagnostic) => diagnostic.severity === "warning").length;
  const previewMessage = result?.error ?? runtimeError ?? "Loading the browser runtime";

  return (
    <div className="home-page">
      <section className="hero-section">
        <div className="hero-intro">
          <p className="eyebrow">Pipeline data preparation with the toolchain included</p>
          <h1>PDL</h1>
          <p>
            A Unix-pipeline-style table DSL that parses, validates, formats, serves editor intelligence, and
            executes deterministic data transforms from one Rust workspace.
          </p>
          <div className="hero-actions">
            <a className="primary-link" href={routeHref("/docs")} onClick={(event) => navigate("/docs", event)}>
              <ArrowRight size={16} aria-hidden="true" />
              Read the quickstart
            </a>
            <a className="secondary-link" href={routeHref("/demos")} onClick={(event) => navigate("/demos", event)}>
              <Code2 size={16} aria-hidden="true" />
              Open demos
            </a>
          </div>
        </div>

        <div className="hero-demo-tool">
          <div className="mini-editor-panel">
            <div className="mini-panel-header">
              <span>
                <Code2 size={16} aria-hidden="true" />
                starter.pdl
              </span>
              <button className="compact-button" type="button" disabled={!runtime} onClick={runCurrent}>
                {running ? <LoaderCircle className="spin" size={15} aria-hidden="true" /> : <Play size={15} aria-hidden="true" />}
                Run
              </button>
            </div>
            <div className="mini-editor-host">
              <PdlEditor diagnostics={diagnostics} files={files} onChange={setSource} runtime={runtime} value={source} />
            </div>
          </div>

          <div className="mini-preview-panel">
            <div className="mini-panel-header">
              <span>
                {running ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <Table2 size={16} aria-hidden="true" />}
                CSV stdout
              </span>
              <MiniStatus state={runtimeState} />
            </div>
            <pre className={`mini-output-stage ${output ? "" : "mini-output-empty"}`}>
              {output || previewMessage}
            </pre>
            <div className={`mini-diagnostics ${errorCount > 0 ? "mini-diagnostics-error" : ""}`}>
              {errorCount} errors, {warningCount} warnings
            </div>
          </div>
        </div>
      </section>

      <section className="install-strip" aria-label="Build PDL">
        <div>
          <p className="eyebrow">Native CLI</p>
          <h2>Build the binary and run a pipeline.</h2>
          <p>Use `check`, `fmt`, `schema`, `plan`, `ir`, `manifest`, and `lsp` while iterating.</p>
        </div>
        <pre>
          <code>{BUILD_COMMANDS}</code>
        </pre>
      </section>

      <section className="install-strip" aria-label="Arrow stream output">
        <div>
          <p className="eyebrow">Streams</p>
          <h2>Prepare tables for downstream consumers.</h2>
          <p>Use Arrow IPC streams when another process needs typed tabular data on stdin.</p>
        </div>
        <pre>
          <code>{INTEROP_COMMANDS}</code>
        </pre>
      </section>

      <section className="language-highlights" aria-label="PDL capabilities">
        <article className="feature-card">
          <h2>Table pipelines</h2>
          <p>Stages are ordered, explicit, and deterministic: load, filter, mutate, aggregate, join, union, sort, and save.</p>
        </article>
        <article className="feature-card">
          <h2>Shared runtime</h2>
          <p>The CLI, LSP, VS Code client, WASM runtime, and browser site call the same Rust language services.</p>
        </article>
        <article className="feature-card">
          <h2>Stream ready</h2>
          <p>Native execution supports clean stdout data streams, including Arrow IPC handoff to downstream tools.</p>
        </article>
      </section>

      <section className="home-band">
        <div>
          <h2>Move from source to prepared data without changing tools</h2>
          <p>
            Use the docs for focused examples, then open the demo route for editable host files, output
            formats, diagnostics, and preset pipelines.
          </p>
        </div>
        <a className="primary-link" href={routeHref("/demos")} onClick={(event) => navigate("/demos", event)}>
          Explore demos
          <ArrowRight size={16} aria-hidden="true" />
        </a>
      </section>
    </div>
  );
}

function MiniStatus({ state }: { state: RuntimeState }): React.ReactElement {
  const label = state === "ready" ? "ready" : state === "loading" ? "loading" : "error";
  return <span className={`mini-status mini-status-${state}`}>WASM {label}</span>;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

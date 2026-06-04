import React from "react";
import {
  AlertCircle,
  CheckCircle2,
  Code2,
  Database,
  LoaderCircle,
  Play,
  RefreshCw,
  Table2,
} from "lucide-react";

import { PdlEditor } from "../PdlEditor";
import type { PdlEditorDiagnostic, PdlRunResult, PdlRuntime } from "../pdlWasm";
import { publicAssetUrl } from "../publicAssets";
import { usePdlRuntime } from "./docs/usePdlRuntime";

interface DemoDataFile {
  file: string;
  label: string;
  rows: number;
  columns: number;
  url: string;
}

interface DemoDataset extends DemoDataFile {
  auxiliaryFiles?: DemoDataFile[];
}

interface PipelinePreset {
  id: string;
  title: string;
  dataset: string;
  summary: string;
  source: string;
  stdoutFormat?: OutputFormat;
}

type OutputFormat = "csv" | "jsonl";
type LoadState = "loading" | "ready" | "error";

interface RunSnapshot {
  source: string;
  dataFile: string;
  filesSignature: string;
  stdoutFormat: OutputFormat;
  result: PdlRunResult;
  editorDiagnostics: PdlEditorDiagnostic[];
}

const DATASETS: Record<string, DemoDataset> = {
  sales: {
    file: "sales.csv",
    label: "Sales",
    rows: 7,
    columns: 5,
    url: publicAssetUrl("data/sales.csv"),
  },
  segments: {
    file: "sales.csv",
    label: "Sales with customers",
    rows: 7,
    columns: 5,
    url: publicAssetUrl("data/sales.csv"),
    auxiliaryFiles: [
      {
        file: "customers.csv",
        label: "Customers",
        rows: 5,
        columns: 2,
        url: publicAssetUrl("data/customers.csv"),
      },
    ],
  },
  orders: {
    file: "orders_raw.csv",
    label: "Raw orders",
    rows: 5,
    columns: 6,
    url: publicAssetUrl("data/orders_raw.csv"),
  },
  daily: {
    file: "daily_orders_2026_02_01.csv",
    label: "Daily orders",
    rows: 2,
    columns: 3,
    url: publicAssetUrl("data/daily_orders_2026_02_01.csv"),
    auxiliaryFiles: [
      {
        file: "daily_orders_2026_02_02.csv",
        label: "Day 2",
        rows: 2,
        columns: 3,
        url: publicAssetUrl("data/daily_orders_2026_02_02.csv"),
      },
    ],
  },
  jsonl: {
    file: "orders.jsonl",
    label: "JSON Lines orders",
    rows: 3,
    columns: 4,
    url: publicAssetUrl("data/orders.jsonl"),
  },
};

const PRESETS: PipelinePreset[] = [
  {
    id: "top-regions",
    title: "Top regions",
    dataset: "sales",
    summary: "Filter, aggregate, sort",
    source: `load "sales.csv"
  | filter status == "completed"
  | group_by region
  | agg
      total_revenue = sum(amount),
      avg_age = mean(customer_age),
      orders = count()
  | sort total_revenue desc
  | limit 3
`,
  },
  {
    id: "segment-revenue",
    title: "Segments",
    dataset: "segments",
    summary: "Binding, join, aggregate",
    source: `let customers =
  load "customers.csv"
  | select customer_id, segment

load "sales.csv"
  | filter status == "completed"
  | join customers on customer_id kind left
  | group_by segment
  | agg revenue = sum(amount), orders = count()
  | sort revenue desc
`,
  },
  {
    id: "clean-orders",
    title: "Clean orders",
    dataset: "orders",
    summary: "String cleanup, mutate",
    source: `load "orders_raw.csv"
  | filter lower(trim(status)) == "completed"
  | mutate
      net_amount = gross_amount - coalesce(discount, 0),
      region_channel = concat(upper(trim(region)), ":", lower(trim(channel))),
      priority = if_else(gross_amount >= 150, "high", "standard")
  | distinct order_id
  | select order_id, region_channel, net_amount, priority
  | sort order_id
`,
  },
  {
    id: "region-summary",
    title: "Region summary",
    dataset: "orders",
    summary: "Reusable cleanup binding",
    source: `let cleaned =
  load "orders_raw.csv"
  | filter lower(trim(status)) == "completed"
  | mutate
      net_amount = gross_amount - coalesce(discount, 0),
      region_channel = concat(upper(trim(region)), ":", lower(trim(channel)))
  | distinct order_id

cleaned
  | group_by region_channel
  | agg orders = count(), revenue = sum(net_amount)
  | sort revenue desc
`,
  },
  {
    id: "windows",
    title: "Windows",
    dataset: "sales",
    summary: "Row-preserving analytics",
    source: `load "sales.csv"
  | filter status == "completed"
  | mutate
      customer_sale_number =
        row_number() over (
          partition_by customer_id
          order_by amount desc
        ),
      customer_revenue =
        sum(amount) over (
          partition_by customer_id
        ),
      region_revenue =
        sum(amount) over (
          partition_by region
        )
  | mutate
      region_revenue_rank =
        dense_rank() over (
          order_by region_revenue desc
        )
  | select
      region,
      customer_id,
      amount,
      customer_sale_number,
      customer_revenue,
      region_revenue_rank
  | sort region_revenue_rank, customer_id, amount desc
`,
  },
  {
    id: "daily-union",
    title: "Daily union",
    dataset: "daily",
    summary: "Union by name, distinct",
    source: `let day2 =
  load "daily_orders_2026_02_02.csv"

load "daily_orders_2026_02_01.csv"
  | union day2 by_name true distinct true
  | sort order_id
`,
  },
  {
    id: "jsonl-orders",
    title: "JSON Lines",
    dataset: "jsonl",
    summary: "JSONL input and output",
    stdoutFormat: "jsonl",
    source: `load "orders.jsonl"
  | filter status == "completed"
  | select order_id, region, amount
  | sort order_id
`,
  },
];

const DEFAULT_PRESET = PRESETS[0];

export function DemoPage(): React.ReactElement {
  const { runtime, error: runtimeError } = usePdlRuntime();
  const [selectedPresetId, setSelectedPresetId] = React.useState(DEFAULT_PRESET.id);
  const selectedPreset = PRESETS.find((preset) => preset.id === selectedPresetId) ?? DEFAULT_PRESET;
  const selectedDataset = DATASETS[selectedPreset.dataset] ?? DATASETS.sales;
  const [source, setSource] = React.useState(DEFAULT_PRESET.source);
  const [stdoutFormat, setStdoutFormat] = React.useState<OutputFormat>(DEFAULT_PRESET.stdoutFormat ?? "csv");
  const [dataTexts, setDataTexts] = React.useState<Record<string, string>>({});
  const [activeFile, setActiveFile] = React.useState(selectedDataset.file);
  const [dataState, setDataState] = React.useState<LoadState>("loading");
  const [dataRevision, setDataRevision] = React.useState(0);
  const [dataError, setDataError] = React.useState<string | null>(null);
  const [snapshot, setSnapshot] = React.useState<RunSnapshot | null>(null);
  const [running, setRunning] = React.useState(false);

  React.useEffect(() => {
    let cancelled = false;
    setDataState("loading");
    setDataError(null);
    setDataTexts({});
    setActiveFile(selectedDataset.file);

    fetchDatasetFiles(selectedDataset)
      .then((texts) => {
        if (cancelled) return;
        setDataTexts(texts);
        setDataState("ready");
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setDataTexts({});
        setDataError(errorMessage(error));
        setDataState("error");
      });

    return () => {
      cancelled = true;
    };
  }, [dataRevision, selectedDataset]);

  const dataFilesSignature = React.useMemo(() => stableFilesSignature(dataTexts), [dataTexts]);
  const fileNames = React.useMemo(() => Object.keys(dataTexts), [dataTexts]);

  const runCurrent = React.useCallback(() => {
    if (!runtime || dataState !== "ready") {
      return;
    }
    const runSource = source;
    const runFiles = dataTexts;
    const runFilesSignature = dataFilesSignature;
    setRunning(true);
    window.setTimeout(() => {
      try {
        setSnapshot(runWithRuntime(runtime, runSource, runFiles, runFilesSignature, selectedDataset.file, stdoutFormat));
      } catch (error: unknown) {
        setSnapshot({
          source: runSource,
          dataFile: selectedDataset.file,
          filesSignature: runFilesSignature,
          stdoutFormat,
          result: { stdout: null, diagnostics: [], error: errorMessage(error) },
          editorDiagnostics: [],
        });
      } finally {
        setRunning(false);
      }
    }, 0);
  }, [dataFilesSignature, dataState, dataTexts, runtime, selectedDataset.file, source, stdoutFormat]);

  React.useEffect(() => {
    if (!runtime || dataState !== "ready") {
      return;
    }
    const timer = window.setTimeout(runCurrent, 240);
    return () => window.clearTimeout(timer);
  }, [dataState, runCurrent, runtime]);

  const selectPreset = React.useCallback((preset: PipelinePreset) => {
    setSelectedPresetId(preset.id);
    setSource(preset.source);
    setStdoutFormat(preset.stdoutFormat ?? "csv");
    setSnapshot(null);
    setDataRevision((revision) => revision + 1);
  }, []);

  const current = Boolean(
    snapshot &&
      snapshot.source === source &&
      snapshot.filesSignature === dataFilesSignature &&
      snapshot.dataFile === selectedDataset.file &&
      snapshot.stdoutFormat === stdoutFormat,
  );
  const result = current ? snapshot?.result ?? null : null;
  const editorDiagnostics = current ? snapshot?.editorDiagnostics ?? [] : [];
  const runtimeDiagnostics = result?.diagnostics ?? [];
  const output = result?.stdout ?? "";
  const hasErrors =
    Boolean(result?.error) ||
    editorDiagnostics.some((diagnostic) => diagnostic.severity === "error") ||
    runtimeDiagnostics.some((diagnostic) => diagnostic.severity === "error");
  const dataDetail = datasetDetail(selectedDataset, dataTexts);

  return (
    <div className="playground-shell">
      <section className="preset-strip" aria-label="Pipeline presets">
        {PRESETS.map((preset) => {
          const dataset = DATASETS[preset.dataset];
          const active = preset.id === selectedPreset.id;
          return (
            <button
              className={`preset-card ${active ? "preset-card-active" : ""}`}
              key={preset.id}
              type="button"
              onClick={() => selectPreset(preset)}
            >
              <span className="preset-title">{preset.title}</span>
              <span className="preset-meta">
                {dataset.label} - {dataset.rows.toLocaleString()} rows
              </span>
              <span className="preset-summary">{preset.summary}</span>
            </button>
          );
        })}
      </section>

      <section className="workspace-grid">
        <div className="pane editor-pane">
          <PaneHeader
            icon={<Code2 size={17} aria-hidden="true" />}
            title="PDL"
            detail={`${source.length} bytes`}
            action={
              <button className="compact-button" type="button" disabled={!runtime || dataState !== "ready"} onClick={runCurrent}>
                {running ? <LoaderCircle className="spin" size={15} aria-hidden="true" /> : <Play size={15} aria-hidden="true" />}
                Run
              </button>
            }
          />
          <PdlEditor diagnostics={editorDiagnostics} files={dataTexts} onChange={setSource} runtime={runtime} value={source} />
        </div>

        <div className="pane data-pane">
          <PaneHeader
            icon={<Database size={17} aria-hidden="true" />}
            title={activeFile || selectedDataset.file}
            detail={dataDetail}
            action={
              <button className="compact-button" type="button" onClick={() => setDataRevision((revision) => revision + 1)}>
                <RefreshCw size={15} aria-hidden="true" />
                Reload
              </button>
            }
          />
          <div className="file-tabs" role="tablist" aria-label="Host files">
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
            aria-label={`${activeFile} data`}
            className="data-input"
            spellCheck={false}
            value={dataTexts[activeFile] ?? ""}
            onChange={(event) =>
              setDataTexts((current) => ({
                ...current,
                [activeFile]: event.target.value,
              }))
            }
          />
        </div>

        <div className="pane output-pane">
          <PaneHeader
            icon={running ? <LoaderCircle className="spin" size={17} aria-hidden="true" /> : <Table2 size={17} aria-hidden="true" />}
            title="Output"
            detail={output ? outputStats(output, stdoutFormat) : "Awaiting run"}
            action={<OutputFormatControl value={stdoutFormat} onChange={setStdoutFormat} />}
          />
          <pre className={`output-stage ${output ? "" : "output-stage-empty"}`}>
            {output || result?.error || runtimeError || dataError || "Loading runtime and data"}
          </pre>
          <DiagnosticsPanel
            editorDiagnostics={editorDiagnostics}
            error={result?.error ?? null}
            hasErrors={hasErrors}
            runtimeDiagnostics={runtimeDiagnostics}
            stale={Boolean(snapshot && !current)}
          />
        </div>
      </section>
    </div>
  );
}

function runWithRuntime(
  runtime: PdlRuntime,
  source: string,
  files: Record<string, string>,
  filesSignature: string,
  dataFile: string,
  stdoutFormat: OutputFormat,
): RunSnapshot {
  const editorResponse = runtime.editorService(source, files, { kind: "diagnostics" });
  return {
    source,
    dataFile,
    filesSignature,
    stdoutFormat,
    result: runtime.run(source, files, stdoutFormat),
    editorDiagnostics: editorResponse.diagnostics,
  };
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
      <div className="pane-detail">{detail}</div>
      {action}
    </div>
  );
}

function OutputFormatControl({
  value,
  onChange,
}: {
  value: OutputFormat;
  onChange: (value: OutputFormat) => void;
}): React.ReactElement {
  return (
    <div className="segmented-control" role="radiogroup" aria-label="Output format">
      {(["csv", "jsonl"] as OutputFormat[]).map((format) => (
        <button
          aria-checked={value === format}
          className={`segmented-option ${value === format ? "segmented-option-active" : ""}`}
          key={format}
          onClick={() => onChange(format)}
          role="radio"
          type="button"
        >
          {format.toUpperCase()}
        </button>
      ))}
    </div>
  );
}

function DiagnosticsPanel({
  editorDiagnostics,
  runtimeDiagnostics,
  error,
  hasErrors,
  stale,
}: {
  editorDiagnostics: PdlEditorDiagnostic[];
  runtimeDiagnostics: PdlRunResult["diagnostics"];
  error: string | null;
  hasErrors: boolean;
  stale: boolean;
}): React.ReactElement {
  if (stale) {
    return (
      <div className="diagnostics diagnostics-pending">
        <LoaderCircle className="spin" size={16} aria-hidden="true" />
        Diagnostics updating
      </div>
    );
  }

  if (!error && editorDiagnostics.length === 0 && runtimeDiagnostics.length === 0) {
    return (
      <div className="diagnostics diagnostics-ok">
        <CheckCircle2 size={16} aria-hidden="true" />
        No diagnostics
      </div>
    );
  }

  return (
    <div className={`diagnostics ${hasErrors ? "diagnostics-error" : "diagnostics-warning"}`}>
      {error ? (
        <div className="diagnostic-row">
          <AlertCircle size={16} aria-hidden="true" />
          <span className="diagnostic-code">Runtime</span>
          <span>{error}</span>
        </div>
      ) : null}
      {editorDiagnostics.map((diagnostic, index) => (
        <div className="diagnostic-row" key={`editor-${diagnostic.code}-${index}`}>
          <AlertCircle size={16} aria-hidden="true" />
          <span className="diagnostic-code">{diagnostic.code}</span>
          <span className="diagnostic-message">
            {diagnostic.message}{" "}
            <span className="diagnostic-span">
              {diagnostic.range.start.line + 1}:{diagnostic.range.start.character + 1}
            </span>
          </span>
        </div>
      ))}
      {runtimeDiagnostics.map((diagnostic, index) => (
        <div className="diagnostic-row" key={`runtime-${diagnostic.code}-${diagnostic.span.start}-${index}`}>
          <AlertCircle size={16} aria-hidden="true" />
          <span className="diagnostic-code">{diagnostic.code}</span>
          <span className="diagnostic-message">
            {diagnostic.message}{" "}
            <span className="diagnostic-span">
              [{diagnostic.span.start}, {diagnostic.span.end})
            </span>
            {diagnostic.help ? <span className="diagnostic-help">{diagnostic.help}</span> : null}
            {diagnostic.related?.map((related, relatedIndex) => (
              <span className="diagnostic-related" key={`${related.span.start}-${relatedIndex}`}>
                {related.message}{" "}
                <span className="diagnostic-span">
                  [{related.span.start}, {related.span.end})
                </span>
              </span>
            ))}
          </span>
        </div>
      ))}
    </div>
  );
}

async function fetchDatasetFiles(dataset: DemoDataset): Promise<Record<string, string>> {
  const files = [dataset, ...(dataset.auxiliaryFiles ?? [])];
  const entries = await Promise.all(
    files.map(async (file) => {
      const response = await fetch(file.url);
      if (!response.ok) {
        throw new Error(`failed to fetch ${file.url}: ${response.status}`);
      }
      return [file.file, await response.text()] as const;
    }),
  );
  return Object.fromEntries(entries);
}

function stableFilesSignature(files: Record<string, string>): string {
  return JSON.stringify(Object.fromEntries(Object.entries(files).sort(([left], [right]) => left.localeCompare(right))));
}

function datasetDetail(dataset: DemoDataset, texts: Record<string, string>): string {
  const primaryText = texts[dataset.file] ?? "";
  const rowCount = estimateRows(primaryText, dataset.file) ?? dataset.rows;
  const linkedCount = dataset.auxiliaryFiles?.length ?? 0;
  return `${rowCount} rows, ${dataset.columns} cols, ${formatBytes(primaryText.length)}${linkedFilesDetail(linkedCount)}`;
}

function linkedFilesDetail(count: number): string {
  if (count === 0) {
    return "";
  }
  return `, +${count} linked file${count === 1 ? "" : "s"}`;
}

function estimateRows(text: string, file: string): number | null {
  if (!text.trim()) {
    return null;
  }
  if (file.endsWith(".jsonl") || file.endsWith(".ndjson")) {
    return text.split(/\r?\n/).filter((line) => line.trim().length > 0).length;
  }
  const lines = text.split(/\r?\n/).filter((line) => line.trim().length > 0);
  return lines.length === 0 ? null : Math.max(0, lines.length - 1);
}

function outputStats(text: string, format: OutputFormat): string {
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

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

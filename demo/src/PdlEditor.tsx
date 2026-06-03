import React from "react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
import "monaco-editor/min/vs/editor/editor.main.css";
import "monaco-editor/esm/vs/editor/contrib/hover/browser/hoverContribution";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import { wireTmGrammars } from "monaco-editor-textmate";
import { Registry } from "monaco-textmate";
import { loadWASM as loadOnigasm } from "onigasm";
import onigasmWasmUrl from "onigasm/lib/onigasm.wasm?url";

import pdlGrammar from "../../editors/vscode/syntaxes/pdl.tmLanguage.json";
import { registerPdlEditorProviders } from "./editorProviders";
import type { PdlEditorDiagnostic, PdlRuntime } from "./pdlWasm";

const LANGUAGE_ID = "pdl";
const SCOPE_NAME = "source.pdl";
const THEME_NAME = "pdl-playground";
const MARKER_OWNER = "pdl-wasm";
const DEFAULT_MODEL_URI = "inmemory://pdl/main.pdl";

let setupPromise: Promise<void> | null = null;
let providerDisposable: monaco.IDisposable | null = null;

interface EditorContext {
  runtime: () => PdlRuntime | null;
  files: () => Record<string, string>;
}

const editorContexts = new Map<string, EditorContext>();

export interface PdlEditorProps {
  value: string;
  files: Record<string, string>;
  diagnostics: PdlEditorDiagnostic[];
  runtime: PdlRuntime | null;
  onChange: (value: string) => void;
  modelUri?: string;
}

export function PdlEditor({ value, files, diagnostics, runtime, onChange, modelUri }: PdlEditorProps): React.ReactElement {
  const hostRef = React.useRef<HTMLDivElement | null>(null);
  const editorRef = React.useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const modelRef = React.useRef<monaco.editor.ITextModel | null>(null);
  const onChangeRef = React.useRef(onChange);
  const diagnosticsRef = React.useRef(diagnostics);
  const filesRef = React.useRef(files);
  const runtimeRef = React.useRef(runtime);
  const [setupError, setSetupError] = React.useState<string | null>(null);
  const resolvedModelUri = React.useMemo(() => monaco.Uri.parse(modelUri ?? DEFAULT_MODEL_URI), [modelUri]);

  React.useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  React.useEffect(() => {
    filesRef.current = files;
  }, [files]);

  React.useEffect(() => {
    runtimeRef.current = runtime;
  }, [runtime]);

  React.useEffect(() => {
    diagnosticsRef.current = diagnostics;
    const model = modelRef.current;
    if (model) {
      setPdlMarkers(model, diagnostics);
    }
  }, [diagnostics]);

  React.useEffect(() => {
    const model = modelRef.current;
    if (model && model.getValue() !== value) {
      const selection = editorRef.current?.getSelection() ?? null;
      model.setValue(value);
      if (selection) {
        editorRef.current?.setSelection(selection);
      }
      setPdlMarkers(model, diagnosticsRef.current);
    }
  }, [value]);

  React.useEffect(() => {
    let cancelled = false;
    let editor: monaco.editor.IStandaloneCodeEditor | null = null;
    let model: monaco.editor.ITextModel | null = null;
    let contentDisposable: monaco.IDisposable | null = null;
    let contextKey: string | null = null;

    setupPdlMonaco()
      .then(() => {
        if (cancelled || !hostRef.current) {
          return;
        }

        ensurePdlProviders();
        model = monaco.editor.createModel(value, LANGUAGE_ID, resolvedModelUri);
        contextKey = model.uri.toString();
        editorContexts.set(contextKey, {
          runtime: () => runtimeRef.current,
          files: () => filesRef.current,
        });
        editor = monaco.editor.create(hostRef.current, {
          model,
          theme: THEME_NAME,
          automaticLayout: true,
          bracketPairColorization: { enabled: true },
          cursorBlinking: "smooth",
          fixedOverflowWidgets: true,
          fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
          fontSize: 13,
          lineHeight: 20,
          minimap: { enabled: false },
          overviewRulerBorder: false,
          padding: { top: 12, bottom: 12 },
          renderLineHighlight: "line",
          scrollBeyondLastLine: false,
          smoothScrolling: true,
          tabSize: 2,
          wordWrap: "off",
        });

        modelRef.current = model;
        editorRef.current = editor;
        setPdlMarkers(model, diagnosticsRef.current);

        contentDisposable = model.onDidChangeContent(() => {
          onChangeRef.current(model?.getValue() ?? "");
        });
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setSetupError(err instanceof Error ? err.message : String(err));
        }
      });

    return () => {
      cancelled = true;
      contentDisposable?.dispose();
      if (contextKey) {
        editorContexts.delete(contextKey);
      }
      if (model) {
        monaco.editor.setModelMarkers(model, MARKER_OWNER, []);
      }
      editor?.dispose();
      model?.dispose();
      if (editorRef.current === editor) {
        editorRef.current = null;
      }
      if (modelRef.current === model) {
        modelRef.current = null;
      }
    };
  }, [resolvedModelUri]);

  return (
    <div className="pdl-editor-shell">
      <div aria-label="PDL source" className="pdl-editor" ref={hostRef} />
      {setupError ? <div className="editor-error">Editor failed to load: {setupError}</div> : null}
    </div>
  );
}

function ensurePdlProviders(): void {
  providerDisposable ??= registerPdlEditorProviders(
    LANGUAGE_ID,
    (model) => editorContexts.get(model.uri.toString())?.runtime() ?? null,
    (model) => editorContexts.get(model.uri.toString())?.files() ?? {},
  );
}

function setupPdlMonaco(): Promise<void> {
  setupPromise ??= (async () => {
    configureMonacoWorker();

    if (!monaco.languages.getLanguages().some((language) => language.id === LANGUAGE_ID)) {
      monaco.languages.register({
        id: LANGUAGE_ID,
        aliases: ["PDL", "pdl"],
        extensions: [".pdl"],
      });
      monaco.languages.setLanguageConfiguration(LANGUAGE_ID, {
        comments: {
          lineComment: "//",
          blockComment: ["/*", "*/"],
        },
        brackets: [
          ["{", "}"],
          ["[", "]"],
          ["(", ")"],
        ],
        autoClosingPairs: [
          { open: "{", close: "}" },
          { open: "[", close: "]" },
          { open: "(", close: ")" },
          { open: '"', close: '"' },
        ],
        surroundingPairs: [
          { open: "{", close: "}" },
          { open: "[", close: "]" },
          { open: "(", close: ")" },
          { open: '"', close: '"' },
        ],
      });
    }

    definePdlTheme();

    await loadOnigasm(onigasmWasmUrl);
    const registry = new Registry({
      getGrammarDefinition: async () => ({
        format: "json",
        content: pdlGrammar,
      }),
    });
    await wireTmGrammars(monaco as unknown as Parameters<typeof wireTmGrammars>[0], registry, new Map([[LANGUAGE_ID, SCOPE_NAME]]));
  })();

  return setupPromise;
}

function configureMonacoWorker(): void {
  const target = globalThis as typeof globalThis & {
    MonacoEnvironment?: monaco.Environment;
  };

  target.MonacoEnvironment ??= {
    getWorker: () => new EditorWorker(),
  };
}

function definePdlTheme(): void {
  monaco.editor.defineTheme(THEME_NAME, {
    base: "vs",
    inherit: true,
    rules: [
      { token: "comment", foreground: "6b7280", fontStyle: "italic" },
      { token: "string", foreground: "7a4a10" },
      { token: "constant.character.escape", foreground: "9f5b00", fontStyle: "bold" },
      { token: "constant.numeric", foreground: "b42318" },
      { token: "constant.language", foreground: "6f42c1" },
      { token: "keyword.control", foreground: "166f5c", fontStyle: "bold" },
      { token: "keyword.operator", foreground: "4f5b63" },
      { token: "support.function.aggregate", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "punctuation", foreground: "68757d" },
    ],
    colors: {
      "editor.background": "#ffffff",
      "editor.foreground": "#171f24",
      "editor.lineHighlightBackground": "#f4f7f6",
      "editorLineNumber.foreground": "#9aa6ac",
      "editorLineNumber.activeForeground": "#21695d",
      "editorCursor.foreground": "#1f6f62",
      "editor.selectionBackground": "#cfe8df",
      "editor.inactiveSelectionBackground": "#e8f2ee",
      "editorIndentGuide.background1": "#edf1f3",
      "editorIndentGuide.activeBackground1": "#c9d8d3",
    },
  });
}

function setPdlMarkers(model: monaco.editor.ITextModel, diagnostics: PdlEditorDiagnostic[]): void {
  monaco.editor.setModelMarkers(
    model,
    MARKER_OWNER,
    diagnostics.map((diagnostic) => diagnosticToMarker(diagnostic)),
  );
}

function diagnosticToMarker(diagnostic: PdlEditorDiagnostic): monaco.editor.IMarkerData {
  const start = fromTextPosition(diagnostic.range.start);
  const end = fromTextPosition(diagnostic.range.end);
  return {
    code: diagnostic.code,
    severity: severityToMarkerSeverity(diagnostic.severity),
    source: "PDL",
    message: diagnostic.message,
    startLineNumber: start.lineNumber,
    startColumn: start.column,
    endLineNumber: end.lineNumber,
    endColumn: end.column === start.column && end.lineNumber === start.lineNumber ? end.column + 1 : end.column,
  };
}

function severityToMarkerSeverity(severity: PdlEditorDiagnostic["severity"]): monaco.MarkerSeverity {
  switch (severity) {
    case "error":
      return monaco.MarkerSeverity.Error;
    case "warning":
      return monaco.MarkerSeverity.Warning;
    case "info":
      return monaco.MarkerSeverity.Info;
    case "hint":
      return monaco.MarkerSeverity.Hint;
  }
}

function fromTextPosition(position: { line: number; character: number }): monaco.IPosition {
  return {
    lineNumber: position.line + 1,
    column: position.character + 1,
  };
}

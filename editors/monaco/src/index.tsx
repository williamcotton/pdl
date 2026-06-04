import React from "react";
import * as monaco from "monaco-editor/esm/vs/editor/editor.api";
import "monaco-editor/min/vs/editor/editor.main.css";
import "monaco-editor/esm/vs/editor/contrib/hover/browser/hoverContribution";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import { wireTmGrammars } from "monaco-editor-textmate";
import { Registry } from "monaco-textmate";
import { loadWASM as loadOnigasm } from "onigasm";
import onigasmWasmUrl from "onigasm/lib/onigasm.wasm?url";

import pdlLanguageConfiguration from "../assets/language-configuration.json";
import pdlGrammar from "../assets/pdl.tmLanguage.json";

export const PDL_LANGUAGE_ID = "pdl";
export const PDL_SCOPE_NAME = "source.pdl";
export const PDL_THEME_NAME = "pdl-playground";
export const PDL_MARKER_OWNER = "pdl-wasm";
export const PDL_DEFAULT_MODEL_URI = "inmemory://pdl/main.pdl";

const SEMANTIC_TOKEN_TYPES = ["keyword", "function", "variable", "string", "number", "operator"];

let setupPromise: Promise<void> | null = null;
let onigasmPromise: Promise<void> | null = null;
let providerDisposable: monaco.IDisposable | null = null;

interface EditorContext {
  runtime: () => PdlEditorRuntime | null;
  files: () => Record<string, string>;
}

const editorContexts = new Map<string, EditorContext>();

export interface TextPosition {
  line: number;
  character: number;
}

export interface TextRange {
  start: TextPosition;
  end: TextPosition;
}

export interface PdlEditorDiagnostic {
  range: TextRange;
  severity: "error" | "warning" | "info" | "hint";
  code: string;
  message: string;
}

export interface PdlCompletion {
  label: string;
  insert_text: string;
  detail: string;
  kind: "Binding" | "Column" | "Format" | "Function" | "Keyword" | "Stage";
}

export interface PdlHover {
  range: TextRange;
  markdown: string;
}

export interface PdlTextEdit {
  range: TextRange;
  new_text: string;
}

export interface PdlSemanticToken {
  range: TextRange;
  token_type: "Keyword" | "Function" | "Variable" | "String" | "Number" | "Operator";
}

export type PdlEditorFeatureRequest =
  | { kind: "diagnostics" }
  | { kind: "hover"; position: TextPosition }
  | { kind: "completion"; position: TextPosition }
  | { kind: "formatting" }
  | { kind: "semanticTokens" }
  | { kind: "documentSymbols" }
  | { kind: "definition"; position: TextPosition }
  | { kind: "references"; position: TextPosition }
  | { kind: "rename"; position: TextPosition; newName: string };

export interface PdlEditorServiceResult<T = unknown> {
  diagnostics: PdlEditorDiagnostic[];
  result: T;
  error: string | null;
}

export interface PdlEditorRuntime {
  editorService<T = unknown>(
    source: string,
    files: Record<string, string>,
    request: PdlEditorFeatureRequest,
    programPath?: string,
  ): PdlEditorServiceResult<T>;
}

export interface PdlEditorProps {
  value: string;
  files: Record<string, string>;
  diagnostics: PdlEditorDiagnostic[];
  runtime: PdlEditorRuntime | null;
  onChange: (value: string) => void;
  modelUri?: string;
  languageId?: string;
  themeName?: string;
  theme?: monaco.editor.IStandaloneThemeData;
  className?: string;
  editorClassName?: string;
  options?: monaco.editor.IStandaloneEditorConstructionOptions;
  setupOptions?: SetupPdlMonacoOptions;
}

export interface RegisterPdlProvidersOptions {
  languageId?: string;
  getRuntime: (model: monaco.editor.ITextModel) => PdlEditorRuntime | null;
  getFiles: (model: monaco.editor.ITextModel) => Record<string, string>;
  programPathForModel?: (model: monaco.editor.ITextModel) => string;
}

export interface SetupPdlMonacoOptions {
  languageId?: string;
  aliases?: string[];
  extensions?: string[];
  scopeName?: string;
  themeName?: string;
  theme?: monaco.editor.IStandaloneThemeData;
  grammar?: unknown;
  languageConfiguration?: monaco.languages.LanguageConfiguration;
  onigasmWasmUrl?: string;
  configureWorker?: boolean;
}

interface PdlLocation {
  range: TextRange;
}

interface PdlDocumentSymbol {
  name: string;
  detail: string;
  kind: "Binding" | "Function" | "Stage";
  range: TextRange;
  selection_range: TextRange;
  children: PdlDocumentSymbol[];
}

export function PdlEditor({
  value,
  files,
  diagnostics,
  runtime,
  onChange,
  modelUri,
  languageId = PDL_LANGUAGE_ID,
  themeName = PDL_THEME_NAME,
  theme,
  className = "pdl-editor-shell",
  editorClassName = "pdl-editor",
  options,
  setupOptions,
}: PdlEditorProps): React.ReactElement {
  const hostRef = React.useRef<HTMLDivElement | null>(null);
  const editorRef = React.useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const modelRef = React.useRef<monaco.editor.ITextModel | null>(null);
  const onChangeRef = React.useRef(onChange);
  const diagnosticsRef = React.useRef(diagnostics);
  const filesRef = React.useRef(files);
  const runtimeRef = React.useRef(runtime);
  const [setupError, setSetupError] = React.useState<string | null>(null);
  const resolvedModelUri = React.useMemo(() => monaco.Uri.parse(modelUri ?? PDL_DEFAULT_MODEL_URI), [modelUri]);

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

    setupPdlMonaco({ ...setupOptions, languageId, themeName, theme })
      .then(() => {
        if (cancelled || !hostRef.current) {
          return;
        }

        ensurePdlProviders(languageId);
        model = monaco.editor.createModel(value, languageId, resolvedModelUri);
        contextKey = model.uri.toString();
        editorContexts.set(contextKey, {
          runtime: () => runtimeRef.current,
          files: () => filesRef.current,
        });
        editor = monaco.editor.create(hostRef.current, {
          model,
          theme: themeName,
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
          ...options,
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
        monaco.editor.setModelMarkers(model, PDL_MARKER_OWNER, []);
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
  }, [languageId, options, resolvedModelUri, setupOptions, theme, themeName]);

  return (
    <div className={className}>
      <div aria-label="PDL source" className={editorClassName} ref={hostRef} />
      {setupError ? <div className="editor-error">Editor failed to load: {setupError}</div> : null}
    </div>
  );
}

export function setupPdlMonaco(options: SetupPdlMonacoOptions = {}): Promise<void> {
  setupPromise ??= setupPdlMonacoOnce(options).catch((error: unknown) => {
    setupPromise = null;
    throw error;
  });
  return setupPromise;
}

export function registerPdlLanguage(options: SetupPdlMonacoOptions = {}): void {
  const languageId = options.languageId ?? PDL_LANGUAGE_ID;
  if (monaco.languages.getLanguages().some((language) => language.id === languageId)) {
    return;
  }
  monaco.languages.register({
    id: languageId,
    aliases: options.aliases ?? ["PDL", "pdl"],
    extensions: options.extensions ?? [".pdl"],
  });
  monaco.languages.setLanguageConfiguration(
    languageId,
    (options.languageConfiguration ?? pdlLanguageConfiguration) as monaco.languages.LanguageConfiguration,
  );
}

export function definePdlTheme(
  themeName = PDL_THEME_NAME,
  theme: monaco.editor.IStandaloneThemeData = defaultPdlTheme(),
): void {
  monaco.editor.defineTheme(themeName, theme);
}

export function defaultPdlTheme(): monaco.editor.IStandaloneThemeData {
  return {
    base: "vs",
    inherit: true,
    rules: [
      { token: "comment", foreground: "6b7280", fontStyle: "italic" },
      { token: "string", foreground: "7a4a10" },
      { token: "number", foreground: "b42318" },
      { token: "keyword", foreground: "166f5c", fontStyle: "bold" },
      { token: "function", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "property", foreground: "9a5512" },
      { token: "variable", foreground: "355f8c" },
      { token: "operator", foreground: "4f5b63" },
      { token: "constant.character.escape", foreground: "9f5b00", fontStyle: "bold" },
      { token: "constant.numeric", foreground: "b42318" },
      { token: "constant.language", foreground: "6f42c1" },
      { token: "invalid.illegal", foreground: "b42318", fontStyle: "underline" },
      { token: "keyword.control", foreground: "166f5c", fontStyle: "bold" },
      { token: "keyword.declaration", foreground: "166f5c", fontStyle: "bold" },
      { token: "keyword.operator.frame", foreground: "7a3f98", fontStyle: "bold" },
      { token: "keyword.operator", foreground: "4f5b63" },
      { token: "support.function", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "support.function.aggregate", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "support.function.aggregate.pdl", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "support.function.scalar", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "support.function.scalar.pdl", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "support.function.window", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "support.function.window.pdl", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "entity.name.function.geometry", foreground: "0f5f8f", fontStyle: "bold" },
      { token: "entity.name.function.stat", foreground: "7a3f98", fontStyle: "bold" },
      { token: "entity.name.function.source", foreground: "3c6b22", fontStyle: "bold" },
      { token: "entity.name.function.literal", foreground: "6f42c1" },
      { token: "entity.name.function", foreground: "315f7d" },
      { token: "variable.parameter.property.unknown", foreground: "a33d2d" },
      { token: "variable.parameter.property", foreground: "9a5512" },
      { token: "variable.other.declaration", foreground: "145f52", fontStyle: "bold" },
      { token: "variable.other.quoted", foreground: "385f70" },
      { token: "variable.other.column", foreground: "355f8c" },
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
  };
}

export function registerPdlEditorProviders(options: RegisterPdlProvidersOptions): monaco.IDisposable {
  const languageId = options.languageId ?? PDL_LANGUAGE_ID;
  const programPathForRequest = options.programPathForModel ?? programPathForModel;
  const disposables: monaco.IDisposable[] = [
    monaco.languages.registerHoverProvider(languageId, {
      provideHover(model, position) {
        const hover = requestFeature<PdlHover | null>(model, options, programPathForRequest, {
          kind: "hover",
          position: toTextPosition(position),
        });
        if (!hover) {
          return null;
        }
        return {
          contents: [{ value: hover.markdown, isTrusted: false, supportHtml: false }],
          range: fromTextRange(hover.range),
        };
      },
    }),
    monaco.languages.registerCompletionItemProvider(languageId, {
      triggerCharacters: ["|", "\"", " "],
      provideCompletionItems(model, position) {
        const completions = requestFeature<PdlCompletion[]>(model, options, programPathForRequest, {
          kind: "completion",
          position: toTextPosition(position),
        });
        return {
          suggestions: (completions ?? []).map((item) => completionItem(item, position)),
        };
      },
    }),
    monaco.languages.registerDocumentFormattingEditProvider(languageId, {
      provideDocumentFormattingEdits(model) {
        const edit = requestFeature<PdlTextEdit | null>(model, options, programPathForRequest, {
          kind: "formatting",
        });
        return edit ? [textEdit(edit)] : [];
      },
    }),
    monaco.languages.registerDocumentSemanticTokensProvider(languageId, {
      getLegend() {
        return {
          tokenTypes: SEMANTIC_TOKEN_TYPES,
          tokenModifiers: [],
        };
      },
      provideDocumentSemanticTokens(model) {
        const tokens = requestFeature<PdlSemanticToken[]>(model, options, programPathForRequest, {
          kind: "semanticTokens",
        });
        return {
          data: new Uint32Array(encodeSemanticTokens(tokens ?? [])),
          resultId: undefined,
        };
      },
      releaseDocumentSemanticTokens() {
        return undefined;
      },
    }),
    monaco.languages.registerDefinitionProvider(languageId, {
      provideDefinition(model, position) {
        const location = requestFeature<PdlLocation | null>(model, options, programPathForRequest, {
          kind: "definition",
          position: toTextPosition(position),
        });
        return location ? [{ uri: model.uri, range: fromTextRange(location.range) }] : [];
      },
    }),
    monaco.languages.registerReferenceProvider(languageId, {
      provideReferences(model, position) {
        const locations = requestFeature<PdlLocation[]>(model, options, programPathForRequest, {
          kind: "references",
          position: toTextPosition(position),
        });
        return (locations ?? []).map((location) => ({
          uri: model.uri,
          range: fromTextRange(location.range),
        }));
      },
    }),
    monaco.languages.registerRenameProvider(languageId, {
      provideRenameEdits(model, position, newName) {
        const edits = requestFeature<PdlTextEdit[]>(model, options, programPathForRequest, {
          kind: "rename",
          position: toTextPosition(position),
          newName,
        });
        return {
          edits: (edits ?? []).map((edit) => ({
            resource: model.uri,
            textEdit: textEdit(edit),
            versionId: undefined,
          })),
        };
      },
    }),
    monaco.languages.registerDocumentSymbolProvider(languageId, {
      provideDocumentSymbols(model) {
        const symbols = requestFeature<PdlDocumentSymbol[]>(model, options, programPathForRequest, {
          kind: "documentSymbols",
        });
        return (symbols ?? []).map(documentSymbol);
      },
    }),
  ];

  return {
    dispose() {
      for (const disposable of disposables) {
        disposable.dispose();
      }
    },
  };
}

export function programPathForModel(model: monaco.editor.ITextModel): string {
  const path = model.uri.path.replace(/^\/+/, "");
  return path ? `memory/${path}` : "memory/main.pdl";
}

export function setPdlMarkers(model: monaco.editor.ITextModel, diagnostics: PdlEditorDiagnostic[]): void {
  monaco.editor.setModelMarkers(
    model,
    PDL_MARKER_OWNER,
    diagnostics.map((diagnostic) => diagnosticToPdlMarker(diagnostic)),
  );
}

export function diagnosticToPdlMarker(diagnostic: PdlEditorDiagnostic): monaco.editor.IMarkerData {
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

function ensurePdlProviders(languageId: string): void {
  providerDisposable ??= registerPdlEditorProviders({
    languageId,
    getRuntime: (model) => editorContexts.get(model.uri.toString())?.runtime() ?? null,
    getFiles: (model) => editorContexts.get(model.uri.toString())?.files() ?? {},
  });
}

async function setupPdlMonacoOnce(options: SetupPdlMonacoOptions): Promise<void> {
  if (options.configureWorker !== false) {
    configureMonacoWorker();
  }
  registerPdlLanguage(options);
  definePdlTheme(options.themeName ?? PDL_THEME_NAME, options.theme ?? defaultPdlTheme());

  await loadOnigasmOnce(options.onigasmWasmUrl ?? onigasmWasmUrl);
  const registry = new Registry({
    getGrammarDefinition: async () => ({
      format: "json",
      content: options.grammar ?? pdlGrammar,
    }),
  });
  await wireTmGrammars(
    monaco as unknown as Parameters<typeof wireTmGrammars>[0],
    registry,
    new Map([[options.languageId ?? PDL_LANGUAGE_ID, options.scopeName ?? PDL_SCOPE_NAME]]),
  );
}

function configureMonacoWorker(): void {
  const target = globalThis as typeof globalThis & {
    MonacoEnvironment?: monaco.Environment;
  };

  target.MonacoEnvironment ??= {
    getWorker: () => new EditorWorker(),
  };
}

function loadOnigasmOnce(url: string): Promise<void> {
  onigasmPromise ??= loadOnigasm(url).catch((error: unknown) => {
    onigasmPromise = null;
    throw error;
  });
  return onigasmPromise;
}

function requestFeature<T>(
  model: monaco.editor.ITextModel,
  options: RegisterPdlProvidersOptions,
  resolveProgramPath: (model: monaco.editor.ITextModel) => string,
  request: PdlEditorFeatureRequest,
): T | null {
  const runtime = options.getRuntime(model);
  if (!runtime) {
    return null;
  }
  const response = runtime.editorService<T>(model.getValue(), options.getFiles(model), request, resolveProgramPath(model));
  if (response.error) {
    console.warn(`PDL editor service failed: ${response.error}`);
    return null;
  }
  return response.result;
}

function toTextPosition(position: monaco.IPosition): TextPosition {
  return {
    line: Math.max(0, position.lineNumber - 1),
    character: Math.max(0, position.column - 1),
  };
}

function fromTextPosition(position: TextPosition): monaco.IPosition {
  return {
    lineNumber: position.line + 1,
    column: position.character + 1,
  };
}

function fromTextRange(range: TextRange): monaco.Range {
  const start = fromTextPosition(range.start);
  const end = fromTextPosition(range.end);
  return new monaco.Range(start.lineNumber, start.column, end.lineNumber, end.column);
}

function completionItem(item: PdlCompletion, position: monaco.IPosition): monaco.languages.CompletionItem {
  return {
    label: item.label,
    kind: completionKind(item.kind),
    detail: item.detail,
    insertText: item.insert_text,
    range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column),
  };
}

function completionKind(kind: PdlCompletion["kind"]): monaco.languages.CompletionItemKind {
  switch (kind) {
    case "Binding":
      return monaco.languages.CompletionItemKind.Variable;
    case "Column":
      return monaco.languages.CompletionItemKind.Field;
    case "Format":
      return monaco.languages.CompletionItemKind.EnumMember;
    case "Function":
      return monaco.languages.CompletionItemKind.Function;
    case "Keyword":
    case "Stage":
      return monaco.languages.CompletionItemKind.Keyword;
  }
}

function textEdit(edit: PdlTextEdit): monaco.languages.TextEdit {
  return {
    range: fromTextRange(edit.range),
    text: edit.new_text,
  };
}

function encodeSemanticTokens(tokens: PdlSemanticToken[]): number[] {
  let previousLine = 0;
  let previousStart = 0;
  const data: number[] = [];

  for (const token of tokens) {
    if (token.range.start.line !== token.range.end.line) {
      continue;
    }
    const deltaLine = token.range.start.line - previousLine;
    const deltaStart = deltaLine === 0 ? token.range.start.character - previousStart : token.range.start.character;
    const length = token.range.end.character - token.range.start.character;
    if (length <= 0) {
      continue;
    }
    data.push(deltaLine, deltaStart, length, semanticTokenIndex(token.token_type), 0);
    previousLine = token.range.start.line;
    previousStart = token.range.start.character;
  }

  return data;
}

function semanticTokenIndex(kind: PdlSemanticToken["token_type"]): number {
  switch (kind) {
    case "Keyword":
      return 0;
    case "Function":
      return 1;
    case "Variable":
      return 2;
    case "String":
      return 3;
    case "Number":
      return 4;
    case "Operator":
      return 5;
  }
}

function documentSymbol(symbol: PdlDocumentSymbol): monaco.languages.DocumentSymbol {
  return {
    name: symbol.name,
    detail: symbol.detail,
    kind: symbolKind(symbol.kind),
    range: fromTextRange(symbol.range),
    selectionRange: fromTextRange(symbol.selection_range),
    tags: [],
    children: symbol.children.map(documentSymbol),
  };
}

function symbolKind(kind: PdlDocumentSymbol["kind"]): monaco.languages.SymbolKind {
  switch (kind) {
    case "Binding":
      return monaco.languages.SymbolKind.Variable;
    case "Function":
      return monaco.languages.SymbolKind.Function;
    case "Stage":
      return monaco.languages.SymbolKind.Method;
  }
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

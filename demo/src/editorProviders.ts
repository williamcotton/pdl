import * as monaco from "monaco-editor/esm/vs/editor/editor.api";

import type {
  PdlCompletion,
  PdlEditorFeatureRequest,
  PdlHover,
  PdlRuntime,
  PdlSemanticToken,
  PdlTextEdit,
  TextPosition,
  TextRange,
} from "./pdlWasm";

const SEMANTIC_TOKEN_TYPES = ["keyword", "function", "variable", "string", "number", "operator"];

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

export function registerPdlEditorProviders(
  languageId: string,
  getRuntime: (model: monaco.editor.ITextModel) => PdlRuntime | null,
  getFiles: (model: monaco.editor.ITextModel) => Record<string, string>,
): monaco.IDisposable {
  const disposables: monaco.IDisposable[] = [
    monaco.languages.registerHoverProvider(languageId, {
      provideHover(model, position) {
        const hover = requestFeature<PdlHover | null>(model, getRuntime, getFiles, {
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
        const completions = requestFeature<PdlCompletion[]>(model, getRuntime, getFiles, {
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
        const edit = requestFeature<PdlTextEdit | null>(model, getRuntime, getFiles, {
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
        const tokens = requestFeature<PdlSemanticToken[]>(model, getRuntime, getFiles, {
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
        const location = requestFeature<PdlLocation | null>(model, getRuntime, getFiles, {
          kind: "definition",
          position: toTextPosition(position),
        });
        return location ? [{ uri: model.uri, range: fromTextRange(location.range) }] : [];
      },
    }),
    monaco.languages.registerReferenceProvider(languageId, {
      provideReferences(model, position) {
        const locations = requestFeature<PdlLocation[]>(model, getRuntime, getFiles, {
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
        const edits = requestFeature<PdlTextEdit[]>(model, getRuntime, getFiles, {
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
        const symbols = requestFeature<PdlDocumentSymbol[]>(model, getRuntime, getFiles, {
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

function requestFeature<T>(
  model: monaco.editor.ITextModel,
  getRuntime: (model: monaco.editor.ITextModel) => PdlRuntime | null,
  getFiles: (model: monaco.editor.ITextModel) => Record<string, string>,
  request: PdlEditorFeatureRequest,
): T | null {
  const runtime = getRuntime(model);
  if (!runtime) {
    return null;
  }
  const response = runtime.editorService<T>(model.getValue(), getFiles(model), request, "memory/main.pdl");
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

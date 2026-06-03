import { publicAssetUrl } from "./publicAssets";

export interface PdlRuntimeDiagnostic {
  code: string;
  severity: "error" | "warning" | "info" | "hint";
  message: string;
  span: {
    start: number;
    end: number;
  };
  related?: Array<{
    span: {
      start: number;
      end: number;
    };
    message: string;
  }>;
  help?: string;
}

export interface PdlRunResult {
  stdout: string | null;
  diagnostics: PdlRuntimeDiagnostic[];
  error: string | null;
}

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

interface PdlWasmExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  pdl_alloc(len: number): number;
  pdl_dealloc(ptr: number, len: number): void;
  pdl_run_json(ptr: number, len: number): bigint;
  pdl_editor_service_json(ptr: number, len: number): bigint;
}

export interface PdlRuntime {
  run(source: string, files: Record<string, string>, stdoutFormat?: string): PdlRunResult;
  editorService<T = unknown>(
    source: string,
    files: Record<string, string>,
    request: PdlEditorFeatureRequest,
    programPath?: string,
  ): PdlEditorServiceResult<T>;
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export async function loadPdlRuntime(url = publicAssetUrl("wasm/pdl.wasm")): Promise<PdlRuntime> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`failed to fetch ${url}: ${response.status}`);
  }

  const instance = await instantiateWasm(response);
  const exports = instance.exports as PdlWasmExports;
  assertPdlExports(exports);

  return {
    run(source, files, stdoutFormat = "csv") {
      return runWithExports(exports, source, files, stdoutFormat);
    },
    editorService<T = unknown>(
      source: string,
      files: Record<string, string>,
      request: PdlEditorFeatureRequest,
      programPath = "memory/main.pdl",
    ) {
      return editorServiceWithExports<T>(exports, source, files, request, programPath);
    },
  };
}

async function instantiateWasm(response: Response): Promise<WebAssembly.Instance> {
  if (WebAssembly.instantiateStreaming) {
    try {
      const result = await WebAssembly.instantiateStreaming(response.clone(), wasmImports());
      return result.instance;
    } catch {
      // Local static servers sometimes serve .wasm with a generic MIME type.
    }
  }

  const bytes = await response.arrayBuffer();
  const result = await WebAssembly.instantiate(bytes, wasmImports());
  return result.instance;
}

function wasmImports(): WebAssembly.Imports {
  return {
    __wbindgen_placeholder__: {
      __wbindgen_object_drop_ref: () => undefined,
      __wbindgen_describe: () => undefined,
      __wbg___wbindgen_throw_9c31b086c2b26051: (ptr: number, len: number) => {
        throw new Error(`wasm-bindgen throw at ${ptr}:${len}`);
      },
    },
    __wbindgen_externref_xform__: {
      __wbindgen_externref_table_set_null: () => undefined,
      __wbindgen_externref_table_grow: () => -1,
    },
  };
}

function runWithExports(
  exports: PdlWasmExports,
  source: string,
  files: Record<string, string>,
  stdoutFormat: string,
): PdlRunResult {
  return callJson<PdlRunResult>(exports, { source, files, stdout_format: stdoutFormat }, exports.pdl_run_json);
}

function editorServiceWithExports<T>(
  exports: PdlWasmExports,
  source: string,
  files: Record<string, string>,
  request: PdlEditorFeatureRequest,
  programPath: string,
): PdlEditorServiceResult<T> {
  return callJson<PdlEditorServiceResult<T>>(
    exports,
    { source, files, program_path: programPath, request },
    exports.pdl_editor_service_json,
  );
}

function callJson<T>(
  exports: PdlWasmExports,
  payload: unknown,
  call: (ptr: number, len: number) => bigint,
): T {
  const inputBytes = encoder.encode(JSON.stringify(payload));
  const inputPtr = exports.pdl_alloc(inputBytes.length);

  try {
    new Uint8Array(exports.memory.buffer, inputPtr, inputBytes.length).set(inputBytes);
    const packed = call(inputPtr, inputBytes.length);
    const outputPtr = Number(packed & 0xffffffffn);
    const outputLen = Number(packed >> 32n);
    const output = new Uint8Array(exports.memory.buffer, outputPtr, outputLen).slice();
    exports.pdl_dealloc(outputPtr, outputLen);
    return JSON.parse(decoder.decode(output)) as T;
  } finally {
    exports.pdl_dealloc(inputPtr, inputBytes.length);
  }
}

function assertPdlExports(exports: WebAssembly.Exports): asserts exports is PdlWasmExports {
  if (
    !(exports.memory instanceof WebAssembly.Memory) ||
    typeof exports.pdl_alloc !== "function" ||
    typeof exports.pdl_dealloc !== "function" ||
    typeof exports.pdl_run_json !== "function" ||
    typeof exports.pdl_editor_service_json !== "function"
  ) {
    throw new Error("pdl.wasm does not expose the expected browser ABI");
  }
}

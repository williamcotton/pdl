import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import onigasmWasmUrl from "onigasm/lib/onigasm.wasm?url";

export const pdlEditorSetupOptions = {
  createEditorWorker: () => new EditorWorker(),
  onigasmWasmUrl,
};

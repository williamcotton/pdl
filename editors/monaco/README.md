# pdl-editor

Reusable Monaco and React editor integration for PDL `0.39.x` browser hosts.

The package owns editor wiring only: language registration, TextMate grammar
setup, the default light theme, marker conversion, Monaco providers, structural
editor-service runtime types, and a thin `<PdlEditor />` component. Hosts keep
runtime loading, execution buttons, output panels, routing, and application
state.

PDL binding and column highlighting is delivered through the shared
editor-service semantic-token ABI exposed by `pdl-wasm` and consumed here by the
Monaco provider. Browser hosts should update these packages instead of adding
PDL-specific parser or token classification logic.

Published packages expose `dist/index.mjs`, `dist/index.cjs`, and
`dist/index.d.ts`, while static TextMate and language-configuration assets stay
available through package subpath exports. Use source mode during local
cross-repo development:

```ts
import { PdlEditor } from "pdl-editor";
import { loadPdlRuntime } from "pdl-wasm";
import EditorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import onigasmWasmUrl from "onigasm/lib/onigasm.wasm?url";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });

const setupOptions = {
  createEditorWorker: () => new EditorWorker(),
  onigasmWasmUrl,
};
```

The editor package keeps Vite-specific `?worker` and `?url` imports out of its
published `dist/` entrypoints. Vite hosts should import those assets in the app
and pass them through `setupOptions`; other bundlers can provide equivalent
worker factories and asset URLs.

Use packed mode before publishing by running `npm pack --dry-run` in
`packages/wasm` and `editors/monaco`, then inspecting the file lists for
`dist/`, declarations, README, package metadata, and editor assets.

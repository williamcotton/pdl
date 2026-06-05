# pdl-editor

Reusable Monaco and React editor integration for PDL `0.28.x` browser hosts.

The package owns editor wiring only: language registration, TextMate grammar
setup, the default light theme, marker conversion, Monaco providers, structural
editor-service runtime types, and a thin `<PdlEditor />` component. Hosts keep
runtime loading, execution buttons, output panels, routing, and application
state.

PDL `0.28.x` binding and column highlighting is delivered through the shared
editor-service semantic-token ABI exposed by `pdl-wasm` and consumed here by the
Monaco provider. Browser hosts should update these packages instead of adding
PDL-specific parser or token classification logic.

Use source mode during local cross-repo development:

```ts
import { PdlEditor } from "pdl-editor";
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

Use packed mode before publishing by running `npm run pack:local` in
`packages/wasm` and `editors/monaco`, then installing the generated tarballs
with `file:` paths in the host app.

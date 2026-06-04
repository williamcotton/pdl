# pdl-editor

Reusable Monaco and React editor integration for PDL `0.27.x` browser hosts.

The package owns editor wiring only: language registration, TextMate grammar
setup, the default light theme, marker conversion, Monaco providers, structural
editor-service runtime types, and a thin `<PdlEditor />` component. Hosts keep
runtime loading, execution buttons, output panels, routing, and application
state.

Use source mode during local cross-repo development:

```ts
import { PdlEditor } from "pdl-editor";
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

Use packed mode before publishing by running `npm run pack:local` in
`packages/wasm` and `editors/monaco`, then installing the generated tarballs
with `file:` paths in the host app.

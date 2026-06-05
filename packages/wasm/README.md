# pdl-wasm

Browser runtime loader and structural TypeScript ABI types for PDL `0.30.x`.

The browser ABI accepts a typed context map for reactive `param` and
`state` declarations, and its editor-service ABI includes parser-backed semantic
token names for table bindings, columns, and context references. Downstream
browser hosts should consume those names
through this package and shared editor integrations such as `pdl-editor` rather
than implementing PDL-specific token classification in TypeScript.

Published packages expose `dist/index.mjs`, `dist/index.cjs`, and
`dist/index.d.ts`, and package tarballs include `dist/pdl.wasm`. During local
source-mode development, build or copy `pdl.wasm` into the host app's public
assets and pass that URL explicitly:

```ts
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

For package-surface validation, run `npm run build:wasm` and
`npm pack --dry-run`; the generated tarball includes the JavaScript entrypoints,
TypeScript declarations, and `dist/pdl.wasm`.

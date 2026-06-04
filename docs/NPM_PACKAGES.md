# PDL Browser Package Development

PDL v0.27 adds package-shaped browser integrations without requiring npm
publication during development.

## Source Mode

Use source mode for daily cross-repo work:

1. Build the local WASM artifact from `pdl/` or use the host app's copy command.
2. Copy `target/wasm32-unknown-unknown/release/pdl_wasm.wasm` into the host's
   public assets as `wasm/pdl.wasm`.
3. Install or alias the sibling packages with filesystem paths:
   - `pdl-wasm`: `file:../pdl/packages/wasm`
   - `pdl-editor`: `file:../pdl/editors/monaco`
4. Load the runtime with an explicit host URL:

```ts
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

The PDL demo and Studio use this mode in this working tree.

## Packed Mode

Use packed mode for package-surface validation before publishing:

1. From `packages/wasm`, run `npm run pack:local`.
2. From `editors/monaco`, run `npm run pack:local`.
3. Install the generated tarballs from `artifacts/` into the demo or Studio
   with `file:` paths.
4. Run the host app's normal type, build, and browser checks.

Generated `dist/` contents, local tarballs, and copied WASM artifacts are
ignored source outputs.

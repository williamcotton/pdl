# PDL Browser Package Development

PDL v0.32 publishes package-shaped browser integrations for npm consumers.

## Published Package Mode

Use published packages for demo, Studio, and downstream package-surface checks:

1. Install the published browser packages:

   ```bash
   npm install pdl-wasm@0.32.0 pdl-editor@0.32.0
   ```

2. Use the package-local WASM asset or pass an explicit host URL. Vite hosts
   should also import Monaco's editor worker and Onigasm's WASM asset from app
   source and pass them to `pdl-editor` through `setupOptions`.

```ts
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

The PDL demo consumes these published package versions.

## Package Validation

Use packed mode for package-surface validation before publishing:

1. From `packages/wasm`, run `npm pack --dry-run`.
2. From `editors/monaco`, run `npm pack --dry-run`.
3. Run the host app's normal type, build, and browser checks against the
   published package versions.

Generated `dist/` contents, local tarballs, and copied WASM artifacts are
ignored source outputs.

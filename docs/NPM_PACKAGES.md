# PDL Browser Package Development

PDL browser integrations are published independently from every Rust/CLI
version bump. Use versions that exist on npm for demo, Studio, and downstream
package-surface checks.

## Published Package Mode

Use published packages for demo, Studio, and downstream package-surface checks:

1. Install the published browser packages:

   ```bash
   npm install pdl-wasm@0.30.0 pdl-editor@0.30.1
   ```

2. Use the package-local WASM asset or pass an explicit host URL. Vite hosts
   should also import Monaco's editor worker and Onigasm's WASM asset from app
   source and pass them to `pdl-editor` through `setupOptions`.

```ts
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

The PDL demo consumes these verified published package versions unless a newer
browser package release has actually been published.

## Package Validation

Use packed mode for package-surface validation before publishing:

1. From `packages/wasm`, run `npm pack --dry-run`.
2. From `editors/monaco`, run `npm pack --dry-run`.
3. Run the host app's normal type, build, and browser checks against the
   published package versions.

Generated `dist/` contents, local tarballs, and copied WASM artifacts are
ignored source outputs.

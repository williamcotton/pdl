# PDL Browser Package Development

PDL browser integrations are published independently from every Rust/CLI
version bump. Use versions that exist on npm for ordinary demo, Studio, and
downstream package-surface checks. During a browser package release, update the
package manifests and consumer pins before publishing, then regenerate or verify
consumer lockfiles after npm has the new tarballs.

## Published Package Mode

Use published packages for demo, Studio, and downstream package-surface checks.
For the v0.40 Rust/CLI release, npm was checked on June 7, 2026:
`pdl-wasm` publishes `0.30.0` and `0.39.0`; `pdl-editor` publishes `0.30.0`,
`0.30.1`, and `0.39.0`. Because no `0.40.0` browser packages are published,
ordinary demo and downstream checks should continue to use the latest verified
published browser package versions:

1. Install the published browser packages:

   ```bash
   npm install pdl-wasm@0.39.0 pdl-editor@0.39.0
   ```

2. Use the package-local WASM asset or pass an explicit host URL. Vite hosts
   should also import Monaco's editor worker and Onigasm's WASM asset from app
   source and pass them to `pdl-editor` through `setupOptions`.

```ts
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

The PDL demo consumes the v0.39 browser package versions during the v0.40
native Rust/CLI release. Do not change `pdl-wasm`, `pdl-editor`, demo consumer
pins, or browser install commands to `0.40.0` unless a browser package release
explicitly prepares and publishes those tarballs.

## Package Validation

Use packed mode for package-surface validation before publishing:

1. From `packages/wasm`, run `npm pack --dry-run`.
2. From `editors/monaco`, run `npm pack --dry-run`.
3. Publish the explicitly prepared `pdl-wasm` and `pdl-editor` versions.
4. Regenerate consumer lockfiles against the published packages.
5. Run the host app's normal type, build, and browser checks against the
   published package versions.

Generated `dist/` contents, local tarballs, and copied WASM artifacts are
ignored source outputs.

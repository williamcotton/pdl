# PDL Browser Package Development

PDL browser integrations are published independently from every Rust/CLI
version bump. Use versions that exist on npm for ordinary demo, Studio, and
downstream package-surface checks. During a browser package release, update the
package manifests and consumer pins before publishing, then regenerate or verify
consumer lockfiles after npm has the new tarballs.

## Published Package Mode

Use published packages for demo, Studio, and downstream package-surface checks.
For the v0.50.0 release, npm was checked on June 11, 2026: `pdl-wasm`
publishes `0.30.0`, `0.39.0`, `0.43.5`, `0.47.0`, and `0.47.1`;
`pdl-editor` publishes `0.30.0`, `0.30.1`, `0.39.0`, `0.43.5`,
`0.43.6`, and `0.47.0`.

The v0.43.5 named-frame release changed the language surface that the browser
packages carry (the WASM parser and the editor grammar assets), so new browser
package versions were prepared and have since been published. Ordinary demo
and downstream checks use the latest
verified published browser package versions:

1. Install the published browser packages:

   ```bash
   npm install pdl-wasm@0.47.1 pdl-editor@0.47.0
   ```

2. Use the package-local WASM asset or pass an explicit host URL. Vite hosts
   should also import Monaco's editor worker and Onigasm's WASM asset from app
   source and pass them to `pdl-editor` through `setupOptions`.

```ts
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

The PDL demo consumes the published `pdl-wasm@0.47.1` / `pdl-editor@0.47.0`
versions. Do not move consumer pins or browser install commands past versions
npm confirms exist; after any future publication, regenerate consumer
lockfiles against the published tarballs.

The v0.44.0 native sink writer release is a native Rust/CLI release with no
browser-visible behavior change. Browser package versions and consumer pins
stay at the published `pdl-wasm@0.43.5` / `pdl-editor@0.43.6`; no `0.44.x`
browser packages are prepared.

The v0.45.0 `pivot_longer`/`complete` native promotion release is likewise a
native Rust/CLI release with no browser-visible behavior change (npm
re-checked June 10, 2026: latest published packages remain `pdl-wasm@0.43.5`
and `pdl-editor@0.43.6`). Browser package versions and consumer pins stay
there; no `0.45.x` browser packages are prepared.

The v0.46.0 byte-backed source-scan release is likewise a native Rust/CLI
release with no browser-visible behavior change: `pdl-wasm` host-byte
execution stays on the row engine. Browser package versions and consumer
pins stay at the published `pdl-wasm@0.43.5` / `pdl-editor@0.43.6`; no
`0.46.x` browser packages are prepared.

The v0.46.5 temporal scalar function release adds `date`, `datetime`,
`year`, `month`, `day`, `date_floor`, and `date_format` to the row
runtime, semantic registry, and editor services that `pdl-wasm` wraps, so
a future browser package release will pick them up. Browser package
publication remains independent of the Rust/CLI release line: consumer
pins stay at the published `pdl-wasm@0.43.5` / `pdl-editor@0.43.6`, and no
`0.46.5` browser packages are prepared.

The v0.47.0 bounded-frame native promotion release is a native Rust/CLI
release with no browser-visible behavior change. Npm was re-checked on
June 11, 2026. Browser package versions and consumer pins may use the
published `pdl-wasm@0.47.1` / `pdl-editor@0.47.0` packages.

The v0.48.0 pipeline-shape native promotion release is likewise a native
Rust/CLI release with no browser-visible execution behavior change. Npm was
re-checked on June 11, 2026: `pdl-wasm@0.48.0` and `pdl-editor@0.48.0` are
not published. Browser package versions and consumer pins stay at the
published `pdl-wasm@0.47.1` / `pdl-editor@0.47.0`; no `0.48.0` browser
packages are prepared.

The v0.49.0 native-coverage completion release is likewise a native Rust/CLI
release with no new parser, editor-service, or WASM-visible language surface.
Npm was re-checked on June 11, 2026: `pdl-wasm@0.49.0` and
`pdl-editor@0.49.0` are not published. Browser package versions and consumer
pins stay at the published `pdl-wasm@0.47.1` / `pdl-editor@0.47.0`; no
`0.49.0` browser packages are prepared.

The v0.50.0 post-parity performance release is likewise a native Rust/CLI
release with no new parser, editor-service, or WASM-visible language surface.
Npm was re-checked on June 11, 2026: `pdl-wasm@0.50.0` and
`pdl-editor@0.50.0` are not published. Browser package versions and consumer
pins stay at the published `pdl-wasm@0.47.1` / `pdl-editor@0.47.0`; no
`0.50.0` browser packages are prepared.

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

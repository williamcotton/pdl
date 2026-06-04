# PDL v0.27 Plan

Status: Implemented
Target version: 0.27.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_26_PLAN.md`](V0_26_PLAN.md)
Downstream consumer: `../studio/docs/V0_3_PLAN.md`
Parallel plan: `../algraf/docs/V0_63_PLAN.md`

## Purpose

PDL v0.27 establishes first-party browser npm packages alongside the existing VS
Code client. The release goal is to stop duplicating static grammar assets,
browser editor wiring, and ad hoc WASM download logic across the PDL demo,
Studio, and external npm consumers.

The VS Code extension should remain a thin package-local language client, but
the TextMate grammar and language configuration should have one canonical source
under `editors/assets/`. A new `editors/monaco/` package should provide the
shared browser editor setup used by the PDL demo and by Studio, and a new
`packages/wasm/` package should provide the browser runtime loader and packaged
WASM artifact for npm users.

This plan is intentionally about editor packaging and integration structure. It
does not add new PDL syntax or runtime semantics.

## Must

- Create canonical editor assets.

  Status: Implemented. `editors/assets/pdl.tmLanguage.json` and
  `editors/assets/language-configuration.json` MUST become the source of truth
  for PDL static highlighting and editor typing behavior.

- Keep the VS Code package self-contained.

  Status: Implemented. The VS Code extension MUST continue to contribute
  `./syntaxes/pdl.tmLanguage.json` and `./language-configuration.json` from
  inside `editors/vscode/`. A sync step MUST copy canonical assets into those
  package-local paths before VS Code compile, lint, test, package, or
  prepublish workflows.

- Add a first-party Monaco integration.

  Status: Implemented. `editors/monaco/` MUST be package-shaped and export the
  `pdl-editor` npm package. It MUST expose the reusable browser editor pieces
  needed by the PDL demo, Studio, and external Monaco hosts: language
  registration, TextMate grammar wiring, language configuration, theme, marker
  conversion, Monaco provider registration, and structural editor-service
  runtime types.

- Add a browser WASM runtime package.

  Status: Implemented. `packages/wasm/` MUST be package-shaped and export the
  `pdl-wasm` npm package. It MUST include TypeScript runtime loader/types and a
  generated `pdl.wasm` artifact in the npm tarball. The generated WASM artifact
  MUST be produced by release/build scripts and MUST NOT be checked into the
  source repository.

- Keep the npm package split explicit.

  Status: Implemented. `pdl-wasm` MUST own browser runtime loading and ABI types.
  `pdl-editor` MUST own Monaco/React editor wiring and declare compatible
  runtime and editor peer dependencies. The two package versions SHOULD align
  with the PDL release version, e.g. `0.27.0`.

- Support unpublished local development.

  Status: Implemented. Cross-repo development MUST NOT require publishing
  `pdl-wasm` or `pdl-editor` to npm. The repo MUST provide a documented local
  workflow with two supported modes:

  - Source mode for daily iteration. The PDL demo and Studio consume sibling
    package source or package build output directly from `../pdl`, while the
    WASM loader receives a caller-provided local `wasmUrl` pointing at a
    generated artifact copied into the host app's public assets.
  - Packed mode for release validation. `pdl-wasm` and `pdl-editor` are built
    and packed into local npm tarballs outside tracked source, then installed
    into the demo or Studio with `file:` paths to approximate a real npm install
    before publishing.

  `npm link` MAY be documented as an advanced option, but source mode and packed
  mode SHOULD be the primary workflows to avoid accidental React or Monaco
  duplicate-dependency issues.

- Keep package APIs narrow and host-friendly.

  Status: Implemented. `pdl-wasm` MUST expose runtime loading, ABI types, and an
  explicit way to load a caller-provided WASM URL or generated local artifact.
  It MUST NOT include Monaco, React, story UI, data panels, or product-specific
  controls. `pdl-editor` MUST expose reusable Monaco setup/provider helpers and
  a thin React editor component that accepts host-provided runtime, files,
  diagnostics, value, change handler, and model URI. It MUST NOT own execution
  buttons, output panels, routing, story state, or WASM build/download policy.

- Provide useful defaults with explicit escape hatches.

  Status: Implemented. The public APIs MUST support the common case with minimal
  setup while still exposing lower-level primitives for custom hosts. Defaults
  should cover the bundled grammar, language configuration, theme, Monaco
  registration, packaged WASM URL, and runtime loader. Escape hatches MUST allow
  callers to provide their own WASM URL, runtime object, Monaco model URI,
  files map, diagnostics, editor options, theme name or theme override, and
  provider registration lifecycle.

- Keep interfaces structural and version-aware.

  Status: Implemented. Editor APIs MUST depend on structural runtime/editor-service
  interfaces rather than concrete demo or Studio modules. Package docs and type
  exports SHOULD make version compatibility explicit so `pdl-editor@0.27.x`
  pairs with a compatible `pdl-wasm@0.27.x` or host-provided runtime.

- Keep language intelligence delegated to PDL services.

  Status: Implemented. The Monaco integration MUST NOT implement PDL parsing,
  analysis, diagnostics, completion, hover, formatting, semantic tokens,
  symbols, definition/reference, or rename in TypeScript. It MUST adapt the
  existing WASM/editor-service ABI into Monaco.

- Update the PDL browser demo to consume the shared Monaco integration.

  Status: Implemented. `demo/` MUST use `editors/monaco/` instead of maintaining a
  parallel local PDL editor component, provider adapter, grammar import, or
  theme definition.

## Should

- Make the npm packages publish-ready.

  Status: Implemented. `packages/wasm/` and `editors/monaco/` should include
  package metadata, export surfaces, and build/typecheck scripts suitable for
  npm publication as `pdl-wasm` and `pdl-editor`. Publishing may be performed in
  v0.27 when release credentials are available, but the repository should remain
  source-first and should not commit generated WASM binaries.

- Preserve app-specific runtime and UI ownership.

  Status: Implemented. `pdl-editor` should own editor wiring only. Host apps should
  continue to own page layout, run buttons, data panels, routing, and
  product-specific chrome. Hosts may use `pdl-wasm` as the default runtime
  loader or provide any structural runtime object that matches the editor-service
  ABI.

- Offer layered APIs.

  Status: Implemented. The packages should expose a high-level React editor for
  common hosts, setup/provider helpers for hosts that already manage Monaco
  models, and runtime loader helpers for hosts that only need PDL execution or
  editor-service calls without a visible editor.

- Provide ergonomic local package iteration.

  Status: Implemented. Local development should support both source-level
  consumption for fast cross-repo edits and package-level validation that
  approximates npm installs before publishing.

- Keep generated local artifacts out of source.

  Status: Implemented. Local package builds, packed npm tarballs, and generated
  WASM artifacts should be written to ignored package `dist/` directories or the
  workspace-level `artifacts/` directory, not committed source paths.

- Preserve model-specific program paths.

  Status: Implemented. Monaco editor-service requests should derive `program_path`
  from each Monaco model URI when a host provides per-file model URIs, while
  still supporting simple single-file demo usage.

- Document downstream consumption.

  Status: Implemented. The PDL docs and release notes should call out that Studio
  v0.3 can consume local sibling sources during development and can later switch
  to `pdl-wasm` and `pdl-editor` packages when they are published.

- Preserve GitHub Release assets for direct consumers.

  Status: Implemented. Existing GitHub Release WASM and VS Code assets should remain
  available for non-npm consumers. npm packages should be an additional
  distribution path, not a replacement for release assets.

## Validation

Required checks before this plan can be marked shipped:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Additional editor validation:

- From `editors/vscode/`, run the asset sync and VS Code package checks.
- From `packages/wasm/`, run the WASM package build/typecheck and verify the
  publish tarball includes `pdl.wasm` while the repository does not track the
  generated binary.
- From `editors/monaco/`, run `pdl-editor` package type/build checks once
  scripts exist.
- Validate at least one unpublished local-consumption workflow for the PDL demo
  and Studio, using sibling source, local package builds, linked packages, or
  local tarballs.
- From `demo/`, run:

  ```bash
  npm run check
  npm run build
  ```

- Browser review should confirm the demo editor highlights v0.27 PDL using the
  shared grammar/theme, and diagnostics, hover, completion, formatting,
  semantic tokens, symbols, definition/reference, and rename still come from the
  upstream PDL editor service.

## Deferred

- Creating an npm organization or scoped package names.
- Moving WASM runtime loaders into the Monaco package.
- A root JavaScript monorepo/workspace, unless package iteration proves it is
  needed.
- Sharing Studio-specific panels, run controls, story state, or layout.
- Any new PDL syntax, runtime, CLI, or file-format behavior.

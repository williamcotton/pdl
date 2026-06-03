# PDL v0.21 Plan

Status: Shipped
Target version: 0.21.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_20_PLAN.md`](V0_20_PLAN.md)

## Purpose

PDL v0.21 shipped after the v0.20 browser documentation and parser recovery
release as a repository automation and packaging release. It keeps the language,
runtime, editor, LSP, WASM ABI, browser demo, and examples stable while making
the VS Code extension and standalone WASM runtime available as CI build
artifacts.

Deferred language and runtime themes remain candidates for later releases:
configurable CSV dialects, comment-preserving formatter rewrites, formatter
style options, LSP code actions/navigation, browser binary output controls,
additional window frame modes, or later mutation-focused language work.

## Must

- Keep v0.21 focused on repository automation and packaging.

  Status: Shipped in 0.21.0. The release adds build artifacts without changing
  source-language semantics, runtime behavior, editor services, or the browser
  WASM ABI.

- Publish CI build artifacts for distributable editor and browser outputs.

  Status: Shipped in 0.21.0. The CI workflow packages the VS Code `.vsix` and
  the standalone browser `pdl.wasm`, verifies that their package versions match
  the Rust workspace WASM crate version, and uploads one VSIX artifact and one
  WASM artifact. Each uploaded artifact contains both a versioned file and a
  `latest` alias.

- Align spec, README, package metadata, and release stamps.

  Status: Shipped in 0.21.0. The workspace version, lockfiles, CLI/version
  output, manifest/language versions, VS Code package metadata, browser demo
  package metadata, README, and `docs/PDL_SPEC.md` are aligned to `0.21.0`.

## Should

- Preserve v0.20 shipped behavior while landing maintenance fixes.

  Status: Shipped in 0.21.0. The spec, code, examples, and docs continue to
  describe the v0.20 language/runtime surface; v0.21 adds CI packaging and
  aligns all release version stamps.

## Deferred

- Comment-preserving formatter rewrites.
- Configurable formatter width or style options.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work.

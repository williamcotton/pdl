# PDL v0.18 Plan

Status: Shipped
Target version: 0.18.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_17_PLAN.md`](V0_17_PLAN.md)

## Purpose

PDL v0.18 is a browser documentation and demo release. The v0.17 browser demo
proved the WASM execution and editor-service ABI in a compact one-screen host;
this release turns it into a fuller public site with a PDL landing page, guided
documentation, runnable examples, bundled data fixtures, and a preset-driven
playground while keeping PDL focused on table preparation.

The release does not add source-language semantics. The native CLI, LSP, VS Code
client, WASM runtime, and browser host continue to share the same Rust parser,
analyzer, driver, executor, and editor services.

## Must

- Replace the minimal browser demo with a routed site.

  Status: Shipped in 0.18.0. The demo now has home, docs, and demos routes that
  work under a Vite base path and static fallback hosting.

- Add live PDL documentation examples.

  Status: Shipped in 0.18.0. The docs pages use the shared WASM runtime and
  Monaco-backed editor-service ABI for diagnostics, hover, completion,
  formatting, semantic tokens, document symbols, definition/reference, and
  rename where those services are available in the browser.

- Bundle deterministic browser data fixtures.

  Status: Shipped in 0.18.0. The demo uses small checked-in CSV and JSON Lines
  fixtures derived from `examples/`, loads them through the browser host, and
  keeps examples runnable without network access.

- Keep browser output behavior accurate.

  Status: Shipped in 0.18.0. CSV and JSON Lines stdout may be previewed in the
  browser. Binary Arrow IPC and Parquet stdout remain native CLI behavior until
  browser binary output controls are promoted in a later release.

- Align spec, README, package metadata, and release stamps.

  Status: Shipped in 0.18.0. The workspace version, lockfiles,
  CLI/version output, manifest/language versions, VS Code package metadata,
  browser demo package metadata, README, and `docs/PDL_SPEC.md` are aligned to
  `0.18.0`.

## Should

- Preserve the language/runtime boundary.

  Status: Shipped in 0.18.0. The React site arranges examples, files, and output
  views, but it must not reimplement PDL parsing, analysis, schema inference, or
  execution semantics in TypeScript.

- Keep the demo useful as a data-prep tool.

  Status: Shipped in 0.18.0. The first screen shows editable PDL source and
  tabular output, and the full demo exposes
  pipeline, input files, output, and diagnostics together.

## Deferred

- Comment-preserving formatter rewrites.
- Configurable formatter width or style options.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work.

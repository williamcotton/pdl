# PDL v0.10 Plan

Status: Complete
Target version: 0.10.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_9_PLAN.md`](V0_9_PLAN.md)

## Purpose

PDL v0.10 is the editor and diagnostic presentation release after the v0.9
missing-pipe recovery release. It keeps the language surface stable while making
CLI diagnostics easier to scan with Rust-like source blocks and making editor
hover output useful for both native LSP clients and browser Monaco/WASM hosts.

## Must

- Add terminal-aware color to human-readable CLI diagnostics in
  `crates/pdl-cli/src/diagnostics.rs`.

  Status: Complete. The CLI now colors severity labels and carets when stderr is
  a terminal and `NO_COLOR` is not set. Plain rendering remains deterministic
  for tests and non-terminal stderr.

- Make human-readable CLI diagnostics look more like Rust diagnostics.

  Status: Complete. The CLI now emits `severity[code]: message`, a
  `--> file:line:column` location line, a guttered source snippet, colored
  primary caret underline, and rustc-style `= help:` / `= note:` lines for
  diagnostic help and related spans.

- Add driver-backed CSV preview hover facts for editor services.

  Status: Complete. Hovering a loaded CSV path such as `"sales.csv"` now shows a
  bounded Markdown preview with columns, derived logical types, nullability, and
  sample rows. Hovering a known column such as `"region"` shows derived type,
  nullability, and sample values. The native LSP path uses `OsDriverIo`; the
  Monaco/WASM path uses the host-supplied in-memory file map through
  `hover_with_driver_io`.

## Deferred

- New stages, formats, commands, editor features, and browser demo expansions
  remain deferred until a maintainer promotes them into this plan with matching
  spec and test scope.

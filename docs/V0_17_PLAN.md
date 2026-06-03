# PDL v0.17 Plan

Status: Shipped
Target version: 0.17.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_16_PLAN.md`](V0_16_PLAN.md)

## Purpose

PDL v0.17 is a formatter readability release. The v0.16 window analytics
syntax made dense `mutate` stages much more common, so this release teaches the
built-in formatter to expand long item lists and window expressions into a
stable, scannable multiline style.

The release does not add new transformation semantics. CLI, LSP, and WASM
formatting all continue to share `pdl-syntax::format_source`.

## Must

- Promote long item-list formatting rules into the formatter and spec.

  Status: Shipped in 0.17.0. The formatter keeps short item lists inline and
  expands long `select`, `drop`, `rename`, `mutate`, `agg`, `sort`, and
  `distinct` item lists into one item per line.

- Format window-heavy `mutate` stages across lines.

  Status: Shipped in 0.17.0. `mutate` stages containing top-level window
  assignments now format with one assignment per line group and each `over (...)`
  clause expanded so `partition_by`, `order_by`, and `rows between ...` are
  visible on separate lines.

- Keep formatter behavior semantic-preserving and shared across hosts.

  Status: Shipped in 0.17.0. CLI `pdl fmt`, LSP formatting, and WASM
  `format_json` still call the syntax crate formatter. Formatter tests cover the
  window-heavy pipeline and idempotence of the expanded output.

- Align release stamps and examples.

  Status: Shipped in 0.17.0. Workspace, lockfile, CLI/version output,
  manifest/language versions, VS Code package manifests, demo manifests, README,
  spec status, and user-facing release strings are aligned to `0.17.0`. The
  customer window metrics example is formatted with the new style.

## Should

- Avoid turning compact programs into overly vertical output.

  Status: Shipped in 0.17.0. Short item-list stages remain inline, preserving
  the familiar leading-pipe style for concise pipelines.

## Deferred

- Comment-preserving formatter rewrites.
- Configurable formatter width or style options.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work.

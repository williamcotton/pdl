# PDL v0.20 Plan

Status: Shipped
Target version: 0.20.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_19_PLAN.md`](V0_19_PLAN.md)

## Purpose

PDL v0.20 shipped as a browser documentation usability release after the v0.19
repository automation release. It keeps the language, runtime, and WASM ABI
surface stable while making the docs route more instructive, especially for
window analytics and input-to-output examples.

Deferred language and runtime themes remain candidates for later releases:
configurable CSV dialects, comment-preserving formatter rewrites, formatter
style options, LSP code actions/navigation, browser binary output controls,
additional window frame modes, or later mutation-focused language work.

## Must

- Improve browser documentation walkthroughs before adding new language surface.

  Status: Shipped in 0.20.0. The docs live examples now show PDL source,
  host-supplied input fixtures, and stdout together, with file tabs for
  multi-input examples. The window analytics page now starts with a whole-table
  rank and builds up through `partition_by`, partition totals, ranking a
  derived window column in a later `mutate`, row frames, running totals, and
  `lag`.

- Fix parser recovery for a binding whose final stage is `sort` without an
  explicit direction before the main pipeline starts from that binding.

  Status: Shipped in 0.20.0. `pdl-syntax` now treats a newline followed by
  a pipeline-start identifier as a sort item boundary, so a valid top-level
  binding reference is not consumed as an invalid sort direction.

## Should

- Prefer one narrow release theme over mixing unrelated feature families.

  Status: Shipped in 0.20.0. The v0.20 slice stayed limited to browser
  documentation usability. Larger language, formatter, LSP, and browser binary
  output themes remain deferred below.

## Deferred

- Comment-preserving formatter rewrites.
- Configurable formatter width or style options.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work.

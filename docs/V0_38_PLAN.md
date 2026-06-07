# PDL v0.38 Plan

Status: Implemented
Target version: 0.38.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_37_PLAN.md`](V0_37_PLAN.md)
Successor plan: [`V0_39_PLAN.md`](V0_39_PLAN.md)
Neighboring Algraf plan: TBD

## Purpose

PDL v0.38 is the successor native-parity release for the remaining v0.37
row-only decisions. v0.37 promoted native `to_number`, `if_else`, path-backed
Arrow IPC file input, Arrow IPC stdin and host-byte inputs, binding-backed
`inner`/`left`/`semi`/`anti` single-key joins, and compatible-schema unions.
v0.38 focuses on the gaps that needed deeper parity design rather than a small
eligibility change. The release promotes the safe native slices and records
explicit row-only decisions for surfaces whose PDL-visible ordering,
formatting, fill, or browser-boundary semantics are more important than raw
native speed.

## Implemented Scope

- Native window analytics ships for row-preserving `mutate` windows with
  `row_number`, `rank`, `dense_rank`, and whole-partition `count`, `sum`,
  `mean`, `min`, and `max`. The native lowering uses Polars
  `GroupsToRows`-style mapping, preserves deterministic ties, supports null
  partition/order keys, supports single-key ordering, and compares against the
  row runtime in parity tests.
- Native join parity now covers `right` and `full` single-key equi-joins in
  addition to the v0.37 `inner`, `left`, `semi`, and `anti` slice. Full joins
  keep PDL row-runtime output order by preserving left-side rows and sorting
  unmatched right-side rows by key.
- Union keeps the v0.37 compatible-schema native slice. Incompatible schemas,
  type coercion beyond existing native supertypes, and null padding remain row
  diagnostics or row-only language decisions rather than new native behavior.
- CSV and JSON Lines writers remain on the row-format writer. Byte-for-byte
  text formatting is PDL-visible, so Polars writers are not used.
- JSON Lines input, `pivot_longer`, and `complete` remain row-only by design.
  Their deterministic schema inference, output order, fill, null, and
  mixed-value semantics are still owned by the row runtime.
- `--engine auto` keeps the existing whole-pipeline eligibility policy. No
  small-data override ships without benchmark evidence strong enough to justify
  a semantic policy branch.

## Row-Only or Slower Features

The following v0.38 features intentionally do not take advantage of Polars and
can remain slower than covered native pipelines:

- Browser/WASM execution. WASM stays row-only and must not pull Polars, Arrow,
  or Parquet into the wasm target graph.
- CSV and JSON Lines text sinks. The row writer preserves stable PDL text
  formatting.
- JSON Lines input. Row inference keeps deterministic mixed-value behavior.
- `pivot_longer` and `complete`. Row execution preserves long-output ordering,
  key expansion, fill expressions, and null behavior.
- Bounded-frame windows, offset/value windows (`lag`, `lead`, `first_value`,
  `last_value`), distribution windows (`percent_rank`, `cume_dist`), non-equi
  joins, and true composite-key join syntax. These stay row-only until a
  narrower native parity design is proven.
- Incompatible-schema union extensions such as language-level null padding or
  broader coercion policy.

## Performance Positioning

v0.38 is a substantial native fast-path release. It expands Polars-backed
execution to the supported scalar expression subset, path-backed and binary IO
slice, compatible unions, single-key equi-joins including `right` and `full`,
and row-preserving windows for `row_number`, `rank`, `dense_rank`, and
whole-partition `count`, `sum`, `mean`, `min`, and `max`.
Eligible pipelines using those features can now stay in the native plan instead
of falling back to row materialization, which is the main performance win for
this release.

The release still keeps the following features row-runtime-only, so they may be
slower than covered Polars-native paths:

- Browser/WASM execution deliberately has no Polars, Arrow, or Parquet in the
  wasm target graph.
- CSV and JSON Lines writers use the row writer to keep PDL's exact text
  formatting stable.
- JSON Lines input uses row inference to preserve deterministic mixed-value and
  schema behavior.
- `pivot_longer` and `complete` use the row runtime to preserve ordering, fill
  expressions, null behavior, and mixed-value semantics.
- Window features outside the v0.38 native subset stay row-only: bounded frames,
  `lag`, `lead`, `first_value`, `last_value`, `percent_rank`, `cume_dist`, and
  multi-key window ordering.
- Joins outside the v0.38 native subset stay row-only: non-equi joins and true
  composite-key join syntax.
- Union extensions beyond compatible schemas stay row-only or diagnostic-only:
  language-level null padding and broader type coercion policy.

## Non-Goals

- Do not expose Polars, Arrow reader, Parquet, or native optimizer internals in
  PDL syntax, CLI public models, editor services, LSP, or WASM.
- Do not make browser/WASM execution native.
- Do not add mid-pipeline native-to-row fallback without a separate design.

## Release Results

- Rust workspace, CLI manifest, VS Code extension, demo app, spec, and native
  coverage docs are bumped to `0.38.0`.
- npm was checked before browser package version changes. `pdl-wasm` publishes
  `0.30.0`; `pdl-editor` publishes `0.30.0` and `0.30.1`. Because no `0.38.0`
  browser packages exist, the browser package manifests and demo dependency
  pins remain on the verified 0.30.x package line.
- WASM dependency checks confirm the `pdl-wasm` target graph does not include
  Polars, Arrow, or Parquet when built without default features.

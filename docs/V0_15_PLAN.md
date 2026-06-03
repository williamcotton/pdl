# PDL v0.15 Plan

Status: Complete / shipped
Target version: 0.15.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_14_PLAN.md`](V0_14_PLAN.md)

## Purpose

PDL v0.15 is the native tabular format parity release after the v0.14 native
CLI introspection release. It promotes the deferred Parquet, Arrow IPC file, and
JSON Lines format work into the existing driver/data/exec boundary without
adding source syntax, stages, expression functions, LSP navigation features, or
browser binary output sinks.

The release thesis is: authors should be able to use the same `load`, `save`,
stdin/stdout, schema, plan, and manifest surfaces across the core native
tabular formats that PDL already names, while preserving stdout discipline and
keeping browser output text-only until a dedicated browser sink release.

## Must

- Promote Parquet native input and output.

  Status: Shipped in 0.15.0. The native data engine reads Parquet schemas and
  tables from file bytes, writes deterministic Parquet bytes from `Table`, and
  supports path inference for `.parquet` and `.pq`. The CLI can load, save, and
  emit Parquet via explicit format names or path inference.

- Promote Arrow IPC file parity.

  Status: Shipped in 0.15.0. `arrow-file` now reads schemas and tables, writes
  Arrow IPC file bytes, sniffs `ARROW1`, and supports `.arrow`/`.feather`
  inference alongside the existing `arrow-stream` support.

- Promote JSON Lines input and output.

  Status: Shipped in 0.15.0. `jsonl`/`ndjson` reads one JSON object per
  non-empty line with deterministic union-of-keys schema order, writes one
  object per row using table column order, and preserves nested JSON values as
  compact JSON text cells rather than flattening them.

- Keep format support shared across analysis, execution, CLI, editor services,
  and WASM host boundaries.

  Status: Shipped in 0.15.0. The semantic format registry now marks Parquet,
  Arrow IPC file, Arrow IPC stream, CSV, and JSON Lines as supported. Native
  execution supports the new formats through `pdl-data`; the WASM run facade
  continues to reject binary stdout formats because its current ABI returns
  UTF-8 text.

- Preserve stdout discipline and diagnostics.

  Status: Shipped in 0.15.0. Native binary stdout remains clean data bytes when
  requested. Human diagnostics still go to stderr. Hosts that disallow binary
  stdout get `E1705` before bytes are emitted.

- Update normative spec, release stamps, README, examples, and tests.

  Status: Shipped in 0.15.0. `docs/PDL_SPEC.md` documents the promoted native
  format behavior and remaining deferrals. Workspace, lockfile, CLI version
  output, VS Code package manifests, browser demo manifests, and README release
  text are aligned to `0.15.0`. Examples and CLI/data tests cover JSON Lines,
  Arrow IPC file, Parquet, path inference, and binary stdout.

## Should

- Keep the data model scalar for this release.

  Status: Shipped in 0.15.0. JSON nested arrays/objects are not flattened and
  do not introduce nested table types. They are represented as compact JSON text
  cells until a later logical-type release expands the table model.

- Avoid broad CSV dialect work.

  Status: Shipped in 0.15.0. CSV delimiter, quote, and null-token configuration
  remain deferred because they require source or CLI option design outside the
  native format parity slice.

## Deferred

- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Window expressions and later mutation-focused language work.
- Projection/filter pushdown and configurable Parquet row group sizing.

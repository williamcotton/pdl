# PDL v0.2 Plan

Status: Shipped
Target version: 0.2.0

## Release Thesis

PDL v0.2 promotes the repository from the initial pre-release slice to a plain
`0.2.0` release line. The release keeps the current CSV-backed execution,
static analysis, LSP, and VS Code client stable while setting the next active
scope for stream/file format expansion and editor/runtime completion.

## Must

### Version Alignment

Status: Landed in 0.2.0.

Drop the pre-release suffix and align Cargo workspace metadata, Cargo.lock, CLI
version output, the normative spec, VS Code package metadata, README references,
and generated package metadata on `0.2.0`.

### Preserve v0.1 Behavior

Status: Landed in 0.2.0.

Keep the existing CSV-backed `run`, `check`, `version`, and `lsp` behavior
working while changing the release version. The version bump must not change the
source language surface or example behavior.

## Should

### Stream Interop Slice

Status: Planned.

Begin the Arrow IPC stdout/stdin path needed for Unix composition with Algraf,
including deterministic stdout behavior and clean separation between diagnostics
on stderr and data on stdout.

### Format Expansion

Status: Planned.

Promote at least one deferred non-CSV file format into implementation scope
after reserving any needed diagnostics and adding runnable examples.

### Editor Completion

Status: Planned.

Round out missing editor-service behaviors such as code actions and broader
navigation while keeping the VS Code extension as a thin client.

## Deferred

- Full Parquet, Arrow IPC file, and JSON Lines parity.
- Stdin stream sniffing beyond the first stream interop slice.
- `mutate`, `join`, `union`, and `distinct`.
- Window expressions, including partitioned ranks, offsets, and running
  aggregates.
- Manifests, `schema`, `plan`, `fmt`, `ast`, and `ir` subcommands.
- WASM and browser demo behavior.
- Full Polars-backed native dataframe execution and optimization.

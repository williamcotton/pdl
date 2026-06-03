# PDL v0.7 Plan

Status: Complete
Target version: 0.7.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_6_PLAN.md`](V0_6_PLAN.md)

## Purpose

PDL v0.7 ships the first browser demo slice after the v0.6 maintenance release.
The release keeps the language surface unchanged and proves the browser boundary:
a Monaco editor sends PDL source plus host-supplied table files to `pdl-wasm`,
receives editor-service diagnostics/features from Rust, and runs the pipeline to
CSV output in memory.

## Must

### Minimal Browser Demo

Status: Complete.

Add a small `demo/` app using the same React, Vite, Monaco, TextMate, Onigasm,
and lucide-react setup as the Algraf demo, but without Algraf's routed site or
chart rendering surface.

Acceptance criteria:

- The first screen is a usable PDL workbench with one `PdlEditor`, one editable
  host-supplied `sales.csv` input, CSV output, and diagnostics.
- Monaco diagnostics and editor features call the `pdl-wasm` editor-service JSON
  ABI. The demo must not implement a TypeScript PDL parser or analyzer.
- The WASM runtime accepts PDL source and a host file map, prepares and executes
  with in-memory driver IO, and returns CSV stdout.
- The host-file request shape is format-neutral even though this release only
  demonstrates CSV bytes.
- Version stamps are bumped to `0.7.0`.

## Deferred

- New stages, dataframe formats beyond the existing CSV implementation, Arrow
  IPC browser output, virtual output sinks, multi-file dataset controls, and
  full editor-service features such as code actions remain deferred until a
  maintainer promotes them into a later plan with matching spec and test scope.

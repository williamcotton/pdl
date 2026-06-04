# PDL v0.24 Plan

Status: Shipped
Target version: 0.24.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_23_PLAN.md`](V0_23_PLAN.md)
Roadmap theme: Browser interoperability for downstream visualization runtimes.

## Purpose

PDL v0.24 extends the browser WASM run ABI so a host can run one PDL story
program and pass its saved text outputs directly to another browser runtime,
such as Algraf, without materializing files on a native filesystem and without
reconstructing save paths in TypeScript.

## Must

- Expose text path-backed `save` outputs from `pdl_run_json`.

  Status: Shipped in 0.24.0. `pdl_run_json` MUST continue to return `stdout`,
  `outputs`, `diagnostics`, and `error` with the existing shapes. It MUST also
  return a `files` object whose keys are logical path-backed `save` sinks from
  named outputs and whose values are UTF-8 text contents encoded using the
  sink's effective output format. CSV and JSON Lines text saves are eligible.
  Binary save sinks remain deferred for a byte-oriented browser ABI.

  Acceptance criteria:

  - The field is additive so existing browser hosts that read only `stdout` or
    `outputs` continue to work.
  - WASM ABI regression tests prove multiple named output saves populate the
    `files` object while no native filesystem files are written.
  - The PDL demo TypeScript type accepts the optional field without changing
    stdout-centric examples.

- Align spec, README, package metadata, and release stamps.

  Status: Shipped in 0.24.0. The workspace version, lockfiles, CLI/version
  output, manifest/language versions, VS Code package metadata, browser demo
  package metadata, README, and `docs/PDL_SPEC.md` are aligned to `0.24.0`.

## Should

- Keep named output tables available alongside virtual saved files.

  Status: Shipped in 0.24.0. Browser hosts that need table-shaped data should keep
  using `outputs`; browser hosts that need virtual downstream files should use
  `files`.

## Validation

Required checks before this plan can be marked shipped:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Additional release validation:

- WASM ABI regression proving multiple text `save` outputs populate `files`.
- Browser demo package build/checks remain clean with the additive field.
- Studio can continue to use `outputs` as fallback while consuming `files` when
  the v0.24 WASM ABI is present.

## Deferred

- Binary virtual file payloads for Arrow IPC, Arrow file, or Parquet saves.
- Output selectors and richer browser output controls.

# PDL v0.25 Plan

Status: Shipped
Target version: 0.25.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_24_PLAN.md`](V0_24_PLAN.md)

## Purpose

PDL v0.25 refreshes the public CLI installation and project overview surfaces
without changing language semantics, native runtime behavior, editor services,
or browser WASM execution.

## Must

- Keep runtime and language behavior stable.

  Status: Shipped in 0.25.0. This release does not add or remove PDL syntax,
  stages, functions, diagnostics, native file formats, CLI commands, LSP
  features, or WASM ABI fields.

- Align spec, README, package metadata, and release stamps.

  Status: Shipped in 0.25.0. The workspace version, lockfiles, CLI/version
  output, manifest/language versions, VS Code package metadata, browser demo
  package metadata, README, and `docs/PDL_SPEC.md` are aligned to `0.25.0`.

## Should

- Refresh public CLI installation and README presentation.

  Status: Shipped in 0.25.0. README and browser demo copy show the supported
  Homebrew tap, install, and upgrade commands, and the README follows the
  Algraf project shape with the demo brand mark, public links, an example tour,
  install/run instructions, browser notes, and workspace layout:

  ```bash
  brew tap williamcotton/pdl
  brew install williamcotton/pdl/pdl
  brew update && brew upgrade williamcotton/pdl/pdl
  ```

- Preserve shipped v0.24 behavior while selecting the next feature slice.

  Status: Shipped in 0.25.0. Browser hosts that consume `stdout`, `outputs`, or
  `files` from `pdl_run_json` should remain compatible unless a future plan item
  explicitly changes the ABI.

## Validation

Required checks before this plan can be marked shipped:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Additional release validation:

- Browser demo package build/checks remain clean with the refreshed homepage
  install section.
- VS Code package metadata remains aligned to the workspace release version.

## Deferred

- Binary virtual file payloads for Arrow IPC, Arrow file, or Parquet saves.
- Output selectors and richer browser output controls.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Additional window frame modes such as range frames and exclude clauses.

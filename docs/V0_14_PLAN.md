# PDL v0.14 Plan

Status: Complete / shipped
Target version: 0.14.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_13_PLAN.md`](V0_13_PLAN.md)

## Purpose

PDL v0.14 is the native CLI introspection release after the v0.13
stream-interoperability release. It promotes the deferred `fmt`, `schema`,
`plan`, `ast`, `ir`, and `manifest` command surface that earlier plans kept out
of scope while the parser, driver, analyzer, and planner APIs stabilized.

The release thesis is: authors should be able to inspect and format the same
program facts that `run`, `check`, LSP, WASM, and the browser demo already use,
without executing table output or weakening stdout discipline. v0.14 does not
add new source syntax, stages, scalar functions, aggregate functions, dataframe
formats, or browser output sinks.

## Must

- Promote native CLI formatting.

  Status: Shipped in 0.14.0. `pdl fmt file.pdl` rewrites source in the stable
  leading-pipe style produced by `pdl-syntax::format_source`.
  `pdl fmt --check file.pdl` reports a non-zero exit code without writing when
  source differs from that style. Formatting remains conservative for documents
  containing comments until comment attachment is source-preserving.

- Promote native schema inspection.

  Status: Shipped in 0.14.0. `pdl schema file.pdl` prints the inferred final
  schema for the main pipeline. `pdl schema file.pdl --binding name` inspects a
  named binding through a CLI-specific analysis target so normal `check` and
  `run` lazy-binding behavior remains unchanged. `--json` emits deterministic
  schema JSON with column names and stage trace summaries.

- Promote native execution-plan inspection.

  Status: Shipped in 0.14.0. `pdl plan file.pdl` prepares and plans the program
  without executing transforms or writing sinks. It reports source reads,
  transform steps, sink/stdout decisions, and selected formats. `--json` emits a
  deterministic machine-readable plan derived from the driver plan and execution
  planner.

- Promote parser and semantic IR inspection.

  Status: Shipped in 0.14.0. `pdl ast file.pdl` prints deterministic JSON for
  the parsed program shape and parse diagnostics. `pdl ir file.pdl` prints
  deterministic JSON for the semantic IR after successful analysis. These
  commands are inspection surfaces only and do not execute data pipelines.

- Promote native manifest inspection.

  Status: Shipped in 0.14.0. `pdl manifest file.pdl` emits deterministic JSON
  for the dry-run manifest surface: implementation version, source path, driver
  inputs, sinks, streams, execution plan, final schema, diagnostics, and Arrow
  stdout Algraf interop hints when applicable. It does not execute transforms or
  write artifacts.

- Preserve stdout discipline and diagnostic behavior.

  Status: Shipped in 0.14.0. Human-readable diagnostics still go to stderr.
  Data stdout remains reserved for `run` data output. Introspection commands own
  their stdout payloads and exit non-zero on diagnostics or unavailable facts.

- Update normative spec, release stamps, README, and tests.

  Status: Shipped in 0.14.0. `docs/PDL_SPEC.md` describes the v0.14 command
  surface and removes native CLI introspection/formatting from the deferred
  implementation list. Workspace, lockfile, CLI version output, VS Code package
  manifests, browser demo manifests, and README release text are aligned to
  `0.14.0`. CLI integration tests cover formatting, schema, plan, AST, IR, and
  manifest commands.

## Should

- Keep JSON shapes deterministic and owned by CLI-facing renderers.

  Status: Shipped in 0.14.0. The CLI renders stable JSON objects with fixed
  field order through `serde_json` and small command-specific views. Lower
  parser, semantic, and driver crates do not gain new serialization
  dependencies just for CLI output.

- Accept existing stream-format overrides where they affect planning.

  Status: Shipped in 0.14.0. `pdl plan` and `pdl manifest` accept
  `--stdin-format` and `--stdout-format` so introspection reflects the same
  stream choices authors use with `run`, while still avoiding stdin reads unless
  execution is requested.

## Deferred

- Parquet, Arrow IPC file parity, JSON Lines, and configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Window expressions and later mutation-focused language work.

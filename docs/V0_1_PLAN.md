# PDL v0.1 Plan

Status: Completed and superseded by `docs/V0_2_PLAN.md`.
Target version: 0.1.0, superseded by 0.2.0.

## Release Thesis

PDL v0.1 establishes the standalone command-line data preparation language:
load deterministic tabular inputs, apply pure pipeline stages, and write stable
artifacts or stdout streams that can feed downstream tools such as Algraf.

The first implementation slice was intentionally narrow. It made the language
runnable with a CSV-backed in-memory engine while preserving the planned crate
boundaries for parser, semantics, data, driver, execution, and CLI work.

## Must

### CLI Alpha

Status: Landed in 0.2.0.

Implement `pdl run`, `pdl check`, and `pdl version` with human diagnostics on
stderr and data output kept clean on stdout.

### CSV Files

Status: Landed in 0.2.0.

Load UTF-8 CSV files with a header row and write deterministic CSV files with
stable column order, LF line endings, and header rows.

### Core Pipeline Stages

Status: Landed in 0.2.0.

Implement deterministic in-memory execution for `load`, `filter`, `select`,
`drop`, `rename`, `group_by`, `agg`, `sort`, `limit`, and `save`.

### Aggregate Functions

Status: Landed in 0.2.0.

Implement `count`, `sum`, `mean`, `min`, and `max` in aggregate context.

### Runnable Examples

Status: Landed in 0.2.0.

Ship small CSV examples that can be run in CI and by users from the repository
root.

## Should

### Shared Crate Boundaries

Status: Landed in 0.2.0.

Keep the initial implementation split across the future runtime crates even
where individual crates are still small.

### Static Analysis

Status: Landed in 0.2.0.

`pdl check` should parse and analyze source without executing pipeline writes,
including cheap CSV header-based schema checks.

### LSP And VS Code

Status: Landed in 0.2.0.

Ship `pdl lsp` as a tower-lsp server backed by `pdl-editor-services`, with
diagnostics, completion, hover, formatting, semantic tokens, document symbols,
and same-document binding navigation/rename. Ship a thin VS Code client under
`editors/vscode/` that spawns `pdl lsp` and contains only client wiring,
settings, TextMate highlighting, language configuration, and packaging metadata.

## Deferred

- Arrow IPC stream input and output.
- Arrow IPC file input and output.
- Parquet input and output.
- JSON Lines input and output.
- Stdin loading and stream sniffing.
- `mutate`, `join`, `union`, and `distinct`.
- Manifests, `schema`, `plan`, `fmt`, `ast`, and `ir` subcommands.
- Full LSP code actions and cross-document navigation beyond same-document
  binding definition/reference/rename.
- WASM and browser demo behavior.
- Full Polars-backed native dataframe execution beyond the initial backend hook.
- Full native `polars` feature set for Arrow, Parquet, JSON, lazy execution,
  strings, temporal data, ranking, and regex.

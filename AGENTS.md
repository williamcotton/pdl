# AGENTS_PDL.md

Guidance for working in a future standalone PDL repository.

## What PDL is

PDL is a Unix-pipeline-style tabular data transformation DSL (file extension
`.pdl`). It loads data from files or stdin, applies deterministic table stages,
and saves or streams the result. A typical PDL program looks like:

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age"
  | sort "total_revenue" desc
  | limit 5
  | save "top_regions.csv"
```

PDL is separate from Algraf, but it is designed to pair well with Algraf. The
preferred interop path is Arrow IPC streaming over stdout/stdin:

```bash
pdl run prep.pdl --stdout-format arrow-stream | algraf render chart.ag --stdin-format arrow-stream --output chart.svg
```

PDL prepares tables. Algraf renders charts. Do not merge their source languages.

## Spec and versioned plans

Three artifacts govern behavior, and they must stay in sync:

1. **`docs/PDL_SPEC.md` — the normative reference.** It describes what the
   implementation does. Read the relevant section before implementing or
   changing behavior. The spec uses RFC-2119-style keywords (`MUST`, `SHOULD`,
   `MAY`, `MUST NOT`) and reserves stable diagnostic codes such as `E1201`;
   honor both. When code intentionally deviates from a `SHOULD`, document why in
   a comment.

2. **`docs/V0_<minor>_PLAN.md` — per-release planning files.** Each release gets
   one. A plan states the release thesis, lists Must/Should items with a
   `Status:` line each, and records deferred work. Plans are guidance, not
   normative: a feature is real only when the spec says `MUST`/`SHOULD` and the
   code implements it. The earliest unreleased plan is the active target.

3. **The code** under `crates/`, plus tests, examples, editor assets, and browser
   demo assets.

### How they tie together

- **Promoting a deferred feature** into a release: add it to the active plan,
  move it into the normative spec section, reserve any new diagnostic codes in
  the spec before implementation, then implement + test + add an example.
- **The spec must match the implementation.** If you ship a stage, function,
  format, CLI flag, manifest field, LSP feature, WASM ABI, or diagnostic,
  document it in the spec in the same change. If you defer something, the spec
  must say so rather than describe it as implemented.
- **Keep examples runnable.** `.pdl` snippets in plans, docs, and README files
  must use syntax and formats the implementation actually accepts.
- **When a plan item lands, update its `Status:` line.** When a release ships,
  start the next `V0_<minor>_PLAN.md`.
- **Create the plan artifact even if work starts first.** Every feature,
  maintenance fix, or release-scoped change must have a current/new plan entry.
  If the active plan is complete or out of scope, start the next minor plan
  rather than editing completed release scope.
- **When a release is completed, align every version stamp.** Update the Cargo
  workspace, lockfile, `docs/PDL_SPEC.md`, VS Code package manifests, browser
  demo manifests, and any generated docs that track the release version.

If the spec, plan, and code disagree, treat it as drift to fix. Reconcile all
three rather than picking one.

### Numbered plan version bumps

Whenever a numbered plan is implemented, bump the repository to that plan's
target version in the same change. Update every related version stamp: Cargo
workspace and lockfile package versions, CLI/version output, `docs/PDL_SPEC.md`,
README or generated docs, VS Code package manifests, browser demo manifests if
present, and user-facing release strings in diagnostics, hovers, or help text.
Mark the implemented plan's `Status:` complete/shipped and start the next minor
plan if the release is now closed.

## Workspace layout

PDL follows the same general architecture shape as Algraf, with `pdl-exec`
instead of a graphics render crate:

| Crate | Responsibility |
| --- | --- |
| `pdl-core` | Shared primitives: `Span`, `Diagnostic`, `Severity`, source IDs |
| `pdl-syntax` | Lexer, parser, AST/CST (rowan), parse diagnostics, formatter |
| `pdl-data` | Dataframe abstraction, logical schemas, CSV/Arrow/Parquet adapters |
| `pdl-driver` | Source/path resolution, format sniffing, schema loading, IO boundary |
| `pdl-semantics` | Name resolution, stage validation, type checking, IR |
| `pdl-exec` | Planning, streaming/blocking execution, writes, manifests, previews |
| `pdl-editor-services` | Completion, hover, tokens, actions, navigation, rename |
| `pdl-lsp` | tower-lsp backend, document cache, LSP transport |
| `pdl-cli` | The `pdl` binary: arg parsing, command dispatch, I/O |
| `pdl-wasm` | Browser/WASM runtime: in-memory `DriverIo`, editor-service ABI |

Recommended repository layout:

```text
pdl/
  Cargo.toml
  crates/
    pdl-cli/
    pdl-core/
    pdl-data/
    pdl-driver/
    pdl-editor-services/
    pdl-exec/
    pdl-lsp/
    pdl-semantics/
    pdl-syntax/
    pdl-wasm/
  docs/
  examples/
  tests/
  editors/
    vscode/
  demo/
  scripts/
```

Dependency direction flows downward. `core` depends on nothing internal.
`driver` depends on syntax, data, and semantics. `editor-services` depends on
syntax and semantics. `lsp` wraps editor-services. `cli` depends on the runtime
stack. `wasm` adapts syntax/semantics/driver/data/exec/editor-services behind a
browser-safe ABI. Do not introduce cycles.

Keep parser, semantics, editor-services, LSP transport, and source language
semantics decoupled from concrete dataframe internals. Polars or other engines
may be used behind `pdl-data`, but must not leak into the language surface.

## Building and running

Build the binary with:

```bash
cargo build -p pdl-cli
```

Try a pipeline while iterating:

```bash
cargo run -p pdl-cli -- check examples/top_regions.pdl
cargo run -p pdl-cli -- run examples/top_regions.pdl
cargo run -p pdl-cli -- run examples/top_regions.pdl --stdout-format arrow-stream > /tmp/out.arrow
```

Core subcommands should include `run`, `check`, `fmt`, `schema`, `plan`, `ast`,
`ir`, `manifest`, `lsp`, and `version` as they land in the spec.

Human-readable logs and diagnostics must go to stderr when stdout is used for
data. Stdout must remain a clean data stream in `--stdout-format` mode.

## Required checks before finishing any change

Run all three from the repo root and make sure they pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

If you touch `editors/vscode/`, also run the extension's package checks from
that directory, once they exist:

```bash
npm install
npm run lint
npm run test
npm run package
```

If you touch `demo/`, also run the demo's package checks from that directory,
once they exist:

```bash
npm install
npm run build
npm run test
```

A change is not done until the relevant checks are clean. Clippy runs over tests
too (`--all-targets`), so keep test code lint-clean.

## VS Code client

`editors/vscode/` is a thin VS Code language client. It does **not** reimplement
PDL language logic. It spawns the `pdl` binary as a language server (`pdl lsp`
by default, configurable via `pdl.server.path` and `pdl.server.args`) and talks
LSP to it.

Expected layout:

```text
editors/vscode/
  package.json
  package-lock.json
  tsconfig.json
  esbuild.js
  src/
    extension.ts
  syntaxes/
    pdl.tmLanguage.json
  language-configuration.json
  README.md
```

Practical implications:

- Improve editor behavior by changing `pdl-editor-services` or `pdl-lsp`, not
  the extension. Touch the extension only for client wiring, activation,
  packaging, settings, and commands.
- The TextMate grammar and `language-configuration.json` are local static editor
  assets. If you add stage keywords, operators, punctuation, or common built-ins,
  update them so static highlighting and bracket/comment behavior track the
  language.
- Keep `package.json`'s `version` aligned with the workspace release version
  when cutting a release.

## Browser demo and WASM

`pdl-wasm` exposes browser-safe entry points over the same Rust parser,
analyzer, driver, exec, and editor-service helpers used by native builds. It is
not a TypeScript implementation of PDL.

The browser ABI should operate on host-supplied in-memory files and streams. It
must not read arbitrary host files, fetch network resources by itself, inspect
process state, or invoke external processes.

`demo/` is a static Monaco host for that ABI. It may show PDL examples, schemas,
plans, CSV/Arrow previews, manifests, and Algraf handoff examples, but it must
not contain a separate parser or analyzer.

## Example generation

Examples live under `examples/`.

Create or update examples when adding new stages, functions, formats, CLI
behavior, or interop behavior.

Examples should be small, deterministic, and runnable in CI. Prefer tiny CSV,
Arrow, or Parquet fixtures that make expected row order and output schema clear.

When adding a new example, update the top-level README or tutorial docs so the
example set stays discoverable. Place examples in tutorial order: loading and
filtering, projection, mutation, grouping and aggregation, joins, streaming,
Algraf handoff, editor/WASM examples.

## Conventions

- Diagnostics are values, not exceptions. Parser, analyzer, driver, data, exec,
  CLI, LSP, and WASM paths return outputs plus `Vec<Diagnostic>` where practical.
  Reserve `panic!` for programmer bugs.
- Every token and syntax node carries a byte-offset `Span`. Spans are half-open
  `[start, end)`. Always test byte offsets with non-ASCII input because bytes
  are not chars and LSP uses UTF-16 columns.
- The lexer/parser are resilient: recover and continue on bad input, emit a
  diagnostic, and never panic.
- Output must be deterministic: stable ordering, stable formatting, no hidden
  time/locale dependence, and no nondeterministic hash-map iteration in emitted
  data or manifests.
- Stdin format handling uses the spec order: explicit format, CLI override,
  extension, magic-byte sniffing, text sniffing, CSV fallback.
- Preserve consumed sniffing bytes. A sniffer must hand the full stream to the
  selected parser.
- PDL source must not execute shell commands, arbitrary code, or network fetches
  by default.

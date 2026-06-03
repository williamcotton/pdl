# PDL v0.4 Plan

Status: Shipped
Target version: 0.4.0

## Release Thesis

PDL v0.4 gives the young repository a durable Rust module layout before the
language and runtime grow much larger. The release maps the current crates onto
the reusable language architecture in `docs/LANGUAGE_ARCH_BLUEPRINT.md` and the
same general organization style that has worked for Algraf, while keeping PDL's
source language, CLI behavior, diagnostics, examples, and public crate contracts
stable.

This is an architecture release, not a source-language feature release. The main
outcome is that future parser, semantic, data, driver, execution, editor, LSP,
CLI, and WASM work has explicit phase and ownership boundaries instead of
collecting indefinitely in crate-root `lib.rs` files.

## Must

### Crate Module Map

Status: Landed in 0.4.0.

Split existing crate-root implementations into small modules with stable
crate-root re-exports. The first pass created modules only where code or a
near-term public boundary exists.

Landed map:

- `pdl-core`: `span`, `diagnostic`, stable diagnostic-code definitions,
  severity, source/position helpers, and shared error type.
- `pdl-syntax`: `lexer`, `cst`, typed `ast` views, `parser`, and `format`
  boundaries.
- `pdl-data`: `value`, `frame`, `schema`, `format`, `csv`, and engine adapter
  boundaries.
- `pdl-driver`: source acquisition, path resolution, injected driver I/O,
  external facts, and phase-tagged preparation reports.
- `pdl-semantics`: analyzer, stage validation, aggregate/stage/function/format
  registries, type/schema model, semantic IR, and lowering boundaries.
- `pdl-exec`: runtime, planning, output handling, manifests, and previews.
- `pdl-editor-services`: shared editor services over syntax plus semantics.
- `pdl-lsp`: backend state and protocol wiring over editor services.
- `pdl-cli`: command dispatch, diagnostics rendering, and command-specific
  handlers, leaving `main.rs` thin.
- `pdl-wasm`: browser-safe ABI plus editor/runtime adapter boundaries without
  host filesystem, network, or process access.

### Behavior Preservation

Status: Landed in 0.4.0.

The refactor preserves v0.3 source-language behavior: accepted syntax,
diagnostics, CLI stdout/stderr behavior, LSP responses, examples, and tests
continue to target the same CSV-backed slice. Visible version strings now report
the 0.4.0 architecture release.

### Syntax Boundary

Status: Landed in 0.4.0.

`pdl-syntax` now has a lossless lexer with trivia tokens and explicit EOF,
parser recovery diagnostics, a rowan CST with composite PDL node kinds, typed AST
views over that CST, and a syntax-owned formatter boundary. The AST views remain
source-shaped; executable meaning is represented in semantic IR.

### Driver Preparation Boundary

Status: Landed in 0.4.0.

`pdl-driver` now owns `SourceOrigin`, source/path resolution, a `DriverIo` trait
with OS and in-memory implementations, external fact storage, and
`PreparationReport` phase diagnostics. Parser and semantic analysis receive
source text/syntax plus facts rather than reading files or inspecting process
state.

### Semantic Purity And IR

Status: Landed in 0.4.0.

`pdl-semantics` now keeps analysis as parsed syntax plus external facts in,
semantic IR plus diagnostics and stage traces out. Filesystem, network,
stdout/stderr, editor, and CLI behavior remain outside semantics.

### Registries For Language Vocabulary

Status: Landed in 0.4.0.

Stage names, aggregate functions, scalar functions, format names, and keywords
now live in `pdl-semantics` registries. Semantic validation, completions, hover,
and format checks share those definitions where practical.

### Execution Planning And Emission Boundary

Status: Landed in 0.4.0.

`pdl-exec` now has a planning step that records sources, transforms, and sinks
without writing bytes. Runtime emission goes through output helpers, and the
single canonical v0.4 backend remains deterministic CSV stdout/file output.

### Public API Compatibility

Status: Landed in 0.4.0.

Crate-root re-exports keep existing workspace APIs usable through the split.
Renames were limited to local module ownership and covered by boundary tests.

### Dependency Direction Audit

Status: Landed in 0.4.0.

The crate graph remains one-way and acyclic. Syntax has no data/execution
dependency; semantics has no driver/execution dependency; editor-services reuse
syntax and semantics; LSP, CLI, VS Code, and WASM remain adapters.

### Adapter Thinness

Status: Landed in 0.4.0.

CLI, LSP, VS Code, and WASM are adapters over shared crates. CLI owns command
parsing, exit codes, stderr/stdout presentation, and dispatch; LSP owns protocol
conversion and document lifecycle; VS Code only spawns/configures the language
server; WASM exposes browser-safe in-memory entry points.

## Should

### Tests Follow Ownership

Status: Landed in 0.4.0.

Focused tests now cover core spans/diagnostics, lexer trivia and byte spans,
parser recovery and CST construction, typed AST views, semantic IR/traces,
driver preparation reports, execution planning, editor services, LSP behavior,
CLI workflows, and WASM ABI helpers.

### Determinism Audit

Status: Landed in 0.4.0.

The split keeps deterministic behavior explicit: source/phase-ordered
diagnostics, stable stage traces, fixed numeric and CSV formatting, deterministic
planning records, and no hidden time/locale dependencies in emitted data or
manifests.

### Documentation Alignment

Status: Landed in 0.4.0.

`docs/PDL_SPEC.md`, README text, release plans, workspace package version,
Cargo lockfile entries, CLI version output, and VS Code package metadata now
identify the reference implementation as 0.4.0.

### Small Algraf Parity Notes

Status: Landed in 0.4.0.

PDL follows Algraf's core/syntax/semantics/driver/editor/LSP/CLI/WASM boundary
style and the rowan CST plus typed view pattern. PDL intentionally diverges at
the runtime boundary: `pdl-exec` plans and emits tabular data, while Algraf's
rendering crate produces graphics. Arrow IPC interop remains a future runtime
format feature, not an Algraf source-language merge.

### Phase Reports For Adapters

Status: Landed in 0.4.0.

Adapters can consume a single preparation report instead of manually merging
parse, load, semantic, and execution diagnostics. The report records diagnostics
by phase in source order.

## Blueprint Implementation Checklist

The v0.4 implementation followed the ordering guidance in
`docs/LANGUAGE_ARCH_BLUEPRINT.md`:

1. Create the package graph and enforce one-way dependencies.
   Status: Landed in 0.4.0.
2. Implement `core` spans and diagnostics first.
   Status: Landed in 0.4.0.
3. Implement a lossless lexer with trivia and EOF.
   Status: Landed in 0.4.0.
4. Implement parser recovery and CST construction.
   Status: Landed in 0.4.0.
5. Add typed AST views over the CST.
   Status: Landed in 0.4.0.
6. Add formatter or source-preserving utilities once the CST is stable.
   Status: Landed in 0.4.0.
7. Define external fact traits and simple in-memory storage.
   Status: Landed in 0.4.0.
8. Add driver I/O trait and source-resolution rules.
   Status: Landed in 0.4.0.
9. Implement semantic IR types before writing the analyzer.
   Status: Landed in 0.4.0.
10. Implement analyzer context and one semantic pass at a time.
    Status: Landed in 0.4.0.
11. Centralize language vocabulary in registries.
    Status: Landed in 0.4.0.
12. Add lowering only after primitive IR is stable.
    Status: Landed in 0.4.0.
13. Add the preparation report and partial-preparation path.
    Status: Landed in 0.4.0.
14. Implement execution planning separately from output emission.
    Status: Landed in 0.4.0.
15. Add one canonical backend first, then optional secondary backends.
    Status: Landed in 0.4.0 for deterministic CSV output; secondary backends
    remain deferred.
16. Build CLI on top of driver plus executor.
    Status: Landed in 0.4.0.
17. Build editor-services on top of syntax plus semantics.
    Status: Landed in 0.4.0.
18. Build LSP as protocol glue only.
    Status: Landed in 0.4.0.
19. Add regression tests at every boundary.
    Status: Landed in 0.4.0.
20. Add spec/docs entries whenever behavior, diagnostics, or public commands
    become real.
    Status: Landed in 0.4.0.

## Deferred

- New source-language stages such as `mutate`, `join`, `union`, and `distinct`.
- Arrow IPC, Parquet, JSON Lines, and stdin stream sniffing feature work.
- A full Algraf-sized parser directory or large placeholder tree before the PDL
  grammar needs it.
- Public API redesigns that are not needed to split existing implementation
  ownership.
- WASM/browser demo behavior beyond preserving browser-safe ABI boundaries.
- Polars/native engine optimization work beyond keeping the adapter boundary
  clear.
- A backend-neutral primitive sink abstraction unless a real second output
  backend needs it.

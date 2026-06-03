# PDL v0.5 Plan

Status: Complete
Target version: 0.5.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_4_PLAN.md`](V0_4_PLAN.md)
Roadmap theme: architecture hardening for PDL's data pipeline.

## Purpose

PDL v0.5 is a refactor-first release. The v0.4 split created the right crate
layout; v0.5 tightens the seams before Arrow, Parquet, stdin sniffing, joins,
mutations, and richer editor features add more weight to the codebase.

Layered Rust workspace design is the architectural template for this release, not a source-language
template. PDL keeps phase ownership discipline, diagnostics-as-values,
driver I/O seam, editor/LSP thinness, and planning-before-emission shape. PDL
intentionally diverges at the runtime boundary: `pdl-exec` executes tabular data
pipelines.

## Release Thesis

v0.5.0 is a **seam and decoupling** release. Its success criterion is a PDL
pipeline stack where future feature work has obvious attachment points:

- parser and typed AST views stay source-shaped and lossless;
- semantic analysis stays pure and Polars-free;
- driver preparation owns source, path, stream, format, and schema facts;
- execution consumes semantic IR plus resolved data plans rather than syntax;
- data engines stay hidden behind `pdl-data` facades;
- CLI, LSP, editor-services, and WASM remain adapters.

No user-visible feature should land in v0.5 unless it is the smallest practical
proof that a boundary works. When a user-visible stage, format, command, ABI, or
diagnostic does land, update `docs/PDL_SPEC.md`, examples, tests, and this plan
in the same change.

## Current Debt Surface

The v0.4 code has the right package graph, but the seams are still early:

- `pdl-driver::DriverIo` currently reads PDL source text and CSV schemas only.
  Future stdin, byte streams, metadata, sniffing, and source dependency
  inventory need a wider but still local-only I/O boundary.
- `pdl-exec::planning` still inspects syntax-level `Program`/`Stage` shapes from
  `PreparedProgram`. Execution planning should consume semantic IR and resolved
  driver facts instead of depending on source AST details.
- `pdl-data` already carries optional Polars/Arrow/Parquet dependencies, but the
  current reference behavior is CSV-only. The public crate surface needs to make
  concrete engine types and native format dependencies hard to leak upward.
- The driver does not yet expose a stable load-free pipeline data plan: sources,
  explicit or inferred formats, stdin/stdout usage, dependencies, and schema
  facts are still implicit in preparation helpers.
- Output emission is CSV-oriented. Future Arrow stream output needs a sink
  boundary that preserves stdout cleanliness without forcing CLI policy into
  `pdl-data` or semantic analysis.
- Editor services are intentionally shared, but future schema-aware editor
  behavior needs a host-injected facts path rather than direct filesystem or
  process reads.

## Scope Rules

- No source-language merge.
- No new source syntax unless the item is explicitly promoted into this plan and
  the spec first.
- Keep existing v0.4 CLI behavior, diagnostics, examples, LSP responses, and
  WASM ABI behavior stable unless a behavior change is explicitly listed here.
- Prefer plan objects, traits, and facades only where they remove concrete
  coupling or prepare an already-specified future feature.
- Do not introduce a broad plugin API, persistent cache, network source,
  environment-variable source, shell-command source, or distributed execution
  model.
- Keep Polars optional and private to `pdl-data` facades from every crate above
  the data layer.
- If a refactor changes generated CSV output, diagnostics, plan JSON, manifest
  JSON, or editor output, treat that as a bug unless the spec and tests are
  updated for an intentional change.

## v0.5.0 Must

### 1. Dependency And Ownership Boundary Audit

Status: Complete.

Audit the current crate graph and public APIs against the intended v0.4/v0.5
architecture.

Acceptance criteria:

- Document the allowed dependency direction in `docs/PDL_SPEC.md` or a short
  architecture note if the current spec is too broad.
- Verify `pdl-syntax` depends only on core syntax concerns and never on data,
  driver, semantics, exec, editor, LSP, CLI, or WASM.
- Verify `pdl-semantics` does not depend on driver, exec, CLI, LSP, editor, or
  WASM crates, and only consumes stable schema/type facts rather than concrete
  dataframe or Polars types.
- Identify every public API above `pdl-data` that mentions concrete data-frame,
  engine, Arrow, Parquet, or Polars implementation types.
- Add focused tests or compile-time checks where practical so future dependency
  drift fails early.

### 2. Driver Source, Stream, And Format Plan

Status: Complete.

Promote driver preparation from "parse plus load CSV schemas" to an explicit
load-free plan for source and stream boundaries.

Acceptance criteria:

- Introduce a driver-owned plan object that records source origin, base
  directory, pipeline input sources, stdin usage, output sinks, explicit format
  names, inferred path formats where available, unresolved sniffing decisions,
  and source spans.
- Plan construction MUST NOT read full data files or consume stdin bytes.
- `DriverIo` grows only the local capabilities needed by the plan and future
  schema loading: path bytes/readers, stdin bytes/readers, path metadata, and
  in-memory host files where appropriate.
- The I/O trait MUST NOT include network access, environment access, shell
  execution, async policy, or cache policy.
- Tests prove plan construction preserves stdin conflict information and
  dependency inventory without reading data bytes.

### 3. Data Engine Facade

Status: Complete.

Make `pdl-data` the only owner of concrete dataframe and native format engine
types.

Acceptance criteria:

- Define stable logical schema/type/table surfaces used by semantics, driver,
  exec, editor services, LSP, CLI JSON, and WASM payloads.
- Keep Polars `DataFrame`, `LazyFrame`, expressions, Arrow reader details, and
  Parquet reader details out of public APIs consumed by parser, semantics,
  editor-services, LSP transport, CLI presentation, and source-language IR.
- Provide small data-layer facade methods for operations `pdl-exec` needs
  rather than having `pdl-exec` construct Polars expressions directly.
- Preserve current CSV behavior and tests.
- If native-format feature gates are adjusted, verify default and WASM-oriented
  builds do not accidentally require native-only dependencies.

### 4. Semantic IR To Execution Plan Handoff

Status: Complete.

Move execution planning off syntax-level stage inspection and onto semantic IR
plus driver facts.

Acceptance criteria:

- `pdl-semantics` produces enough IR and stage transition metadata for
  `pdl-exec` to plan execution without inspecting typed syntax AST nodes.
- `pdl-exec` owns physical/executable planning: streaming versus blocking
  classification, source reads, transformation steps, sink writes, stdout
  output, manifest summaries, and execution limits.
- `pdl-exec` may depend on `pdl-driver` for prepared driver facts, but it SHOULD
  NOT depend on source AST node types for ordinary planning.
- Existing `run`, `check`, and test behavior remains stable.
- Add a regression test proving the plan records the same v0.4 operations while
  being built from semantic/driver outputs.

### 5. Phase Reports And Diagnostic Ownership

Status: Complete.

Keep every adapter on one phase-tagged diagnostic/report path.

Acceptance criteria:

- The preparation/report model includes parse, driver/source, schema/facts,
  semantic, planning, execution, and output phases, even if some phases are
  empty in v0.5.
- Recoverable phases can continue so editor and `check` surfaces report useful
  downstream diagnostics without hiding the original source/facts failure.
- Diagnostic codes emitted by any changed code path are already reserved in
  `docs/PDL_SPEC.md`.
- CLI human output, CLI JSON output, LSP diagnostics, WASM payloads, tests, and
  manifests all derive from the same core diagnostic values.
- Tests cover phase ordering and non-ASCII byte-span to UTF-16 conversion for at
  least one diagnostic path touched by the refactor.

### 6. Adapter Thinness Audit

Status: Complete.

Keep CLI, LSP, VS Code, editor-services, and WASM as adapters rather than
parallel implementations of PDL behavior.

Acceptance criteria:

- CLI owns argument parsing, exit codes, stdout/stderr policy, and file writes;
  language behavior stays in syntax/driver/semantics/exec.
- LSP owns protocol conversion and document lifecycle only.
- Editor-services owns protocol-neutral completion, hover, formatting, tokens,
  document symbols, navigation, and rename helpers over shared syntax/semantics
  state.
- VS Code remains a thin client that spawns/configures `pdl lsp`.
- WASM uses in-memory driver/exec host boundaries and MUST NOT read arbitrary
  host files, process stdin, environment variables, network resources, or
  external processes.

### 7. Spec, Plan, And Validation Hygiene

Status: Complete.

Keep the planning artifact, normative spec, code, examples, and version stamps
aligned as refactor work lands.

Acceptance criteria:

- `docs/PDL_SPEC.md` is updated alongside any shipped public API, command,
  diagnostic, format, ABI, or semantic behavior change.
- This plan's `Status:` lines are updated as each item lands, is deferred, or
  is rejected.
- Examples remain runnable and deterministic.
- Workspace, lockfile, VS Code package metadata, WASM/demo metadata, README,
  and spec status are aligned when the release is completed.
- Required checks pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

## v0.5.0 Should

### Architecture Audit Note

Status: Complete.

Capture the architecture lessons that matter for PDL without making future
contributors re-audit unrelated repositories.

Acceptance criteria:

- Record what PDL keeps: lossless syntax, diagnostics-as-values,
  driver I/O seam, preparation reports, editor/LSP thinness, WASM host boundary,
  deterministic outputs, and planning-before-emission.
- Record where PDL intentionally diverges: tabular execution instead of graphics
  rendering, Polars/data-engine privacy, Arrow/stdout source-sink discipline,
  and no source-language merge.
- Call out unrelated coupling to avoid copying blindly, especially render-level
  asset loading through driver policy and concrete dataframe maps across runtime
  seams.

### Source And Sink Boundary Sketches For Arrow

Status: Complete.

Prepare the interfaces for Arrow IPC without shipping Arrow user workflows in
this refactor release unless explicitly promoted later.

Acceptance criteria:

- Sketch source and sink descriptors for `csv`, `arrow-stream`, `arrow-file`,
  `parquet`, and `jsonl` in the driver/exec boundary.
- Keep sniffing and Arrow byte parsing deferred unless promoted to Must scope.
- Tests may use fake byte formats or compact fixtures to prove descriptor
  routing, but should not claim Arrow support unless real Arrow readers/writers
  are implemented and documented.

### Schema Cache And Preview Boundary

Status: Complete.

Design where lightweight schema caching and preview data will live once editor
features need them.

Acceptance criteria:

- Cache keys use resolved source identity plus a fingerprint or host-provided
  version, never path strings alone.
- The cache stores schemas and load errors, not full frames, unless a later
  plan explicitly promotes frame caching.
- LSP/editor paths can opt out or use host-provided schemas when runtime data is
  caller-provided.

## Explicitly Deferred Past v0.5.0

- Real Arrow IPC stream input/output unless promoted with spec, code, tests, and
  examples.
- Parquet loading beyond interface preparation.
- JSON Lines loading.
- Stdin stream sniffing implementation.
- New source-language stages: `mutate`, `join`, `union`, `distinct`, and window
  expressions.
- Full `schema`, `plan`, `manifest`, `ast`, `ir`, and `fmt` CLI subcommands
  beyond interface cleanup unless promoted.
- Browser demo product work beyond preserving WASM-safe boundaries.
- Persistent caches, full-frame caches, render/result caches, or query-driven
  compilation.
- Network, URL, command, environment-variable, or remote database sources.
- A plugin API for stages, functions, formats, or execution engines.

## Promotion Workflow

1. Add guard tests around current v0.4 CLI, LSP, WASM, examples, diagnostics,
   and CSV output before moving a boundary.
2. Audit dependencies and public APIs; remove or document concrete engine leaks.
3. Introduce driver source/stream/format plan objects without reading data.
4. Tighten `pdl-data` facades and feature gates.
5. Move execution planning from syntax inspection to semantic IR plus driver
   facts.
6. Keep reports and diagnostics on one phase-tagged path.
7. Update the spec for any public behavior that becomes real.
8. Run formatter, clippy, workspace tests, and any touched editor/WASM checks.

# Language Architecture Blueprint

This document extracts the reusable architecture pattern from this repository,
without depending on Algraf-specific charting semantics. It is meant to be
handed to another LLM or engineer as a blueprint for building a new language
implementation with clean boundaries from input to output.

If the implementation is not Rust, treat "crate" as "package" or "module".

## Core Principles

1. Keep each compiler/runtime phase in its own package.
2. Make dependency direction one-way and explicit.
3. Represent diagnostics as ordinary values, not exceptions.
4. Preserve exact source structure in syntax while lowering meaning into a
   separate IR.
5. Keep filesystem, process, network, and editor concerns outside parser and
   semantic analysis.
6. Make semantic analysis pure: parsed tree plus external facts in, IR plus
   diagnostics out.
7. Use small registries for language vocabulary so validation, completion,
   hover, and documentation agree.
8. Split execution into planning and emission so every output backend consumes
   the same resolved scene/model.
9. Keep output deterministic: stable ordering, stable formatting, no implicit
   locale/time dependence.
10. Test every boundary independently, then test the full pipeline through the
    public adapters.

## Package Layout

Use this package graph as the default architecture:

```text
core
  -> syntax
  -> data/runtime facts
  -> semantics
  -> driver
  -> executor/output
  -> editor-services
  -> lsp
  -> cli
  -> wasm/embedded adapters
```

Recommended packages:

- `core`: shared primitives such as spans, diagnostics, severity, stable codes,
  and small source-position helpers. It depends on no internal package.
- `syntax`: lexer, parser, concrete syntax tree, typed AST views, parser
  diagnostics, formatter, and source-expression extraction helpers.
- `data` or `facts`: external facts needed for semantic analysis and execution,
  such as schemas, tables, project metadata, module indexes, or runtime inputs.
- `semantics`: name resolution, scope analysis, type checking, validation,
  lowering from AST to semantic IR, and semantic diagnostics. No I/O.
- `driver`: orchestration for source resolution, variable expansion, injected
  I/O, loading external facts, running parse plus analysis, and building a
  phase-tagged report.
- `executor` or `render`: runtime execution, compilation, rendering, codegen, or
  whatever converts the semantic IR plus loaded facts into final output.
- `editor-services`: completion, hover, symbols, semantic tokens, formatting,
  code actions, inlay hints, and navigation. It reuses syntax and semantics.
- `lsp`: protocol wrapper around editor-services. It should not implement
  language logic.
- `cli`: command-line parsing, command dispatch, diagnostics presentation,
  output file selection, and process exit codes. It should not implement
  language logic.
- `wasm` or `embedded`: in-memory driver I/O and public browser/runtime API.

The important rule is that UI adapters depend on the language pipeline, not the
other way around.

## End-To-End Flow

### 1. Source Acquisition

The adapter receives input from a file, stdin, inline source text, an editor
buffer, or an embedded API call.

Represent source origin explicitly:

```text
SourceInput =
  | Stdin
  | Inline { label }
  | Path(path)
```

The source origin is used only for diagnostics, path resolution, and caching.
Parser and semantic analysis receive plain source text or syntax trees.

### 2. Lexing

The lexer converts source text into a lossless token stream:

```text
TokenWithSpan {
  kind: TokenKind,
  span: Span,
  text: original_lexeme
}
```

Guidelines:

- Spans are byte offsets, half-open: `[start, end)`.
- Keep whitespace and comments as trivia tokens.
- Always append an explicit EOF token.
- Lexical errors produce error tokens plus diagnostics and then continue.
- Preserve original lexeme text so formatting, source maps, and diagnostics can
  remain precise.

Core types:

```text
Span { start: ByteOffset, end: ByteOffset }
Diagnostic {
  code,
  severity,
  message,
  span,
  related_spans,
  help
}
TokenKind
TokenWithSpan
LexResult { tokens, diagnostics }
```

### 3. Parsing

The parser should build a lossless concrete syntax tree (CST), not just an
owned semantic AST.

Recommended approach:

- Use recursive descent for block/call/declaration structure.
- Use a Pratt parser or precedence-climbing parser for expressions.
- Build a green tree or other immutable CST that preserves trivia and malformed
  regions.
- Record parse diagnostics as values.
- Recover locally on errors so one bad construct does not discard later valid
  constructs.
- Never panic on malformed user input.

Parser result:

```text
Parse {
  syntax_tree,
  diagnostics
}
```

Keep syntax kinds centralized:

```text
SyntaxKind =
  trivia tokens
  literal/identifier tokens
  punctuation tokens
  contextual keyword tokens
  composite nodes
  error nodes
```

### 4. Typed AST Views

Layer lightweight typed views over the CST instead of building a second
source-shaped tree.

Example shape:

```text
Root
DocumentHeader
TopLevelItem
Block
Declaration
Call
Argument
ValueExpr
Expression
Identifier
ErrorNode
```

Each typed view should:

- Wrap a syntax node.
- Cast only when the node has the expected `SyntaxKind`.
- Walk children on demand.
- Return optional values where recovery may have inserted or omitted nodes.
- Provide helpers for names, spans, arguments, literal values, and child items.

This keeps the CST authoritative for formatting and editor features, while the
semantic analyzer can consume ergonomic typed views.

### 5. Source Resolution And External Fact Loading

Keep source resolution and external loading in the driver, not in syntax or
semantics.

Use a narrow I/O trait:

```text
DriverIo {
  read_path(path) -> bytes
  read_stdin() -> bytes
  metadata(path) -> metadata
  optional domain-specific loaders
}
```

The default OS implementation reads real files. Tests, editor previews, WASM,
and embedded use in-memory implementations.

Represent resolved inputs separately from source syntax:

```text
DataLocation =
  | Path { path, format }
  | Input { format }
  | DomainSpecific { path, options }
```

The driver owns:

- base-directory rules,
- stdin conflict rules,
- CLI data overrides,
- source-format overrides,
- schema cache lookup,
- loading primary inputs,
- loading named inputs,
- converting load errors into diagnostics.

### 6. External Fact Model

Semantic analysis usually needs facts about the outside world: table schemas,
module exports, known types, project config, dependency metadata, etc.

Expose those facts through narrow traits and simple definitions:

```text
ColumnDef {
  name,
  dtype,
  nullable,
  examples
}

Table {
  schema() -> [ColumnDef]
  row_count() -> usize
  value(name, row) -> ValueRef
  column(name) -> ColumnView
}
```

The important pattern is the boundary: downstream packages should depend on a
trait such as `Table`, not on the concrete in-memory storage. Concrete storage
can be columnar, row-based, lazy, or replaced later without changing parser,
semantics, editor, or output interfaces.

### 7. Semantic Analysis

Semantic analysis consumes typed AST views plus loaded external facts and
returns:

```text
Analysis {
  ir: Option<ProgramIr>,
  diagnostics: Vec<Diagnostic>
}
```

It should be pure:

- No filesystem reads.
- No process execution.
- No network.
- No CLI/editor assumptions.
- No output serialization.

Use one analyzer context threaded through focused passes:

```text
Analyzer {
  primary_facts,
  named_facts,
  derived_facts,
  scopes,
  reserved_names,
  synthetic_name_counter,
  diagnostics
}
```

Recommended passes:

1. Collect declarations that must be visible regardless of source order.
2. Analyze top-level arguments and defaults.
3. Build lexical/block scopes.
4. Resolve names against active scopes and external facts.
5. Type-check expressions and property/argument values.
6. Validate declarations against registries.
7. Build normalized IR.
8. Desugar high-level constructs into primitive IR when useful.
9. Use invalid/unknown sentinels to avoid cascading failures.

### 8. Registries

Keep language vocabulary in registries instead of scattering string checks.

Registry examples:

```text
CallDef {
  name,
  accepted_args,
  required_args,
  docs,
  lowering_kind
}

PropertySpec {
  name,
  key,
  accepted_value_forms,
  required
}
```

Use the same registries for:

- semantic validation,
- completion,
- hover/signature help,
- documentation generation,
- tests that ensure docs and implementation agree.

### 9. Semantic IR

The IR should represent executable meaning, not source shape.

Good IR properties:

- All names that can resolve are resolved.
- Types are attached to resolved references.
- Defaults are made explicit.
- High-level syntax sugar is lowered or marked for lowering.
- Invalid or unknown constructs are represented with sentinel values.
- Source spans remain attached for diagnostics.
- Ordering is source-stable where output ordering matters.

Example IR skeleton:

```text
ProgramIr {
  source,
  imports_or_inputs,
  declarations,
  derived_items,
  global_options,
  root_items
}

BlockIr {
  data_or_context_ref,
  frame_or_scope,
  layers_or_statements,
  local_options,
  span
}

ReferenceIr {
  name,
  type,
  span
}

CallIr {
  kind,
  mappings_or_args,
  settings,
  span
}
```

For a non-graphics language, replace "layers", "frame", and "settings" with the
language's own executable concepts. Keep the separation between source AST and
semantic IR.

### 10. Lowering

Put high-level desugaring in semantics if it depends only on schema/types and
source structure. Put it in execution if it needs actual runtime rows or values.

Semantic lowering can:

- allocate synthetic names,
- create synthetic derived declarations,
- rewrite high-level constructs into primitive constructs,
- preserve spans pointing to the original source call,
- reject unsupported combinations before execution.

Keep lowered IR explicit so execution does not need to understand every syntax
shortcut.

### 11. Preparation Report

Adapters should not manually assemble parse, load, semantic, and execution
diagnostics. Centralize that in the driver:

```text
ReportPhase =
  | Parse
  | Load
  | Semantic
  | Execute

PreparationReport {
  diagnostics: [(ReportPhase, Diagnostic)]
  structured_warnings
}
```

This lets CLI, LSP, tests, and embedded APIs surface the same errors in the same
order.

### 12. Execution Or Output Planning

The executor receives IR plus loaded facts and builds a fully resolved plan.

For a renderer, that plan is a scene. For a compiler, it might be an executable
module. For an interpreter, it might be an evaluated program state. The common
rule is:

```text
IR + facts -> planned model + diagnostics
```

Planning should resolve:

- derived data or derived declarations,
- scopes that require runtime values,
- domains and scales,
- layouts or allocation decisions,
- dependency ordering,
- resource budgets,
- warnings that depend on actual data,
- metadata for downstream consumers.

Do not write output bytes during planning.

### 13. Emission Boundary

After planning, hand the planned model to a closed set of output backends:

```text
OutputBackend {
  type Output
  emit(planned_model, metadata, diagnostics) -> Output
}
```

Backends should not make semantic, layout, or scale decisions. They only
serialize the planned model.

This pattern enables multiple outputs that agree by construction:

- canonical text output,
- JSON model,
- raster/image output,
- bytecode,
- debug output,
- embedded runtime output.

### 14. Backend-Neutral Primitive Sink

If output emits many primitive operations, define a backend-neutral sink:

```text
PrimitiveSink {
  open_group(role)
  close_group()
  begin_item(metadata)
  end_item()
  primitive_a(...)
  primitive_b(...)
  text(...)
}
```

Then every backend observes the same primitive calls. This avoids duplicating
geometry/codegen logic across SVG, canvas JSON, raster, or debug backends.

### 15. CLI Adapter

The CLI owns:

- command-line parsing,
- reading source text,
- variable expansion,
- invoking driver preparation,
- invoking execution/output,
- writing files/stdout,
- converting diagnostics into human or JSON output,
- process exit codes,
- optional strict mode.

The CLI should not:

- tokenize or parse manually,
- implement language validation,
- know editor protocol details,
- duplicate source-resolution logic,
- manually inspect concrete data storage unless it is presenting a command
  specifically about data.

### 16. Editor And LSP Adapter

Editor services should be a protocol-free package. LSP should be a thin wrapper.

Editor-services package owns:

- document snapshots,
- UTF-16/byte-offset conversion,
- diagnostics publication data,
- completions from registries and syntax context,
- hover from AST/IR/facts,
- semantic tokens,
- code actions,
- formatting,
- navigation and references.

LSP package owns:

- JSON-RPC transport,
- request/response method wiring,
- document cache lifecycle,
- cancellation and async boundaries,
- converting editor-service results into LSP types.

The editor client should only spawn/connect to the server and configure it.

### 17. Diagnostics

Use stable diagnostic codes from day one.

Diagnostic rules:

- Every diagnostic has a stable code.
- Codes are registered centrally.
- Codes are documented.
- Production code uses constants, not raw string literals.
- Diagnostics carry primary spans.
- Related spans explain duplicate declarations, shadowing, or cross-reference
  problems.
- Help text is optional and concise.
- Warnings do not block output unless strict mode promotes them.

Useful severity model:

```text
Severity =
  | Error
  | Warning
  | Information
  | Hint
```

### 18. Determinism

Make deterministic behavior a design constraint, not a cleanup task.

Practical rules:

- Use stable maps when output order matters.
- Sort sets before emitting.
- Use fixed numeric formatting.
- Avoid current time and locale in output.
- Keep diagnostics in source or phase order.
- Keep synthetic names deterministic.
- Keep caches invalidated by explicit fingerprints.

### 19. Testing Strategy

Test each phase as its own product:

- Lexer tests: token kinds, spans, trivia, invalid characters, non-ASCII byte
  offsets.
- Parser tests: valid constructs, malformed recovery, CST shape, diagnostics.
- AST tests: typed accessors over partial and malformed syntax.
- Data/facts tests: schema inference, value access, null semantics, format
  loading, cache behavior.
- Semantic tests: name resolution, scope, type checking, registry validation,
  duplicate handling, unknown sentinel behavior, lowering.
- Driver tests: path resolution, injected I/O, stdin conflicts, report phase
  ordering, partial preparation.
- Executor tests: planning, runtime warnings, budget limits, derived outputs.
- Backend tests: golden output, deterministic formatting, backend parity.
- Adapter tests: CLI commands, LSP diagnostics/completion/hover, embedded API.

For language examples, run them through the full pipeline and keep rendered or
compiled artifacts in sync when applicable.

## Implementation Checklist For Another LLM

Ask the implementing LLM to follow this order:

1. Create the package graph and enforce one-way dependencies.
2. Implement `core` spans and diagnostics first.
3. Implement a lossless lexer with trivia and EOF.
4. Implement parser recovery and CST construction.
5. Add typed AST views over the CST.
6. Add formatter or source-preserving utilities once the CST is stable.
7. Define external fact traits and simple in-memory storage.
8. Add driver I/O trait and source-resolution rules.
9. Implement semantic IR types before writing the analyzer.
10. Implement analyzer context and one semantic pass at a time.
11. Centralize language vocabulary in registries.
12. Add lowering only after primitive IR is stable.
13. Add the preparation report and partial-preparation path.
14. Implement execution planning separately from output emission.
15. Add one canonical backend first, then optional secondary backends.
16. Build CLI on top of driver plus executor.
17. Build editor-services on top of syntax plus semantics.
18. Build LSP as protocol glue only.
19. Add regression tests at every boundary.
20. Add spec/docs entries whenever behavior, diagnostics, or public commands
    become real.

## Anti-Patterns To Avoid

- Parser directly reads files or loads project data.
- Semantic analysis depends on CLI flags or editor state.
- LSP reimplements validation or completion logic instead of calling shared
  services.
- Output backends recompute layout or semantic decisions.
- Diagnostics are raw strings without stable codes.
- Invalid syntax aborts parsing instead of recovering.
- AST and IR are the same type.
- Concrete data storage leaks into parser, semantics, or editor packages.
- Tests only cover the CLI and miss phase-level behavior.
- Documentation describes features before code and tests actually implement
  them.


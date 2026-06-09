# PDL Detailed Specification

Status: Draft 0.43.0
Audience: implementers, language designers, data engineers, runtime engineers, LSP authors, WASM host authors, VS Code extension authors, test authors, and streaming consumers
Scope: standalone Unix-pipeline-style DSL for deterministic tabular data loading, transformation, aggregation, streaming, and materialization

## Current Reference Implementation Status

The current repository implementation line is `0.43.0`, tracked in
`docs/V0_43_PLAN.md`. Version 0.43.0 is a test-infrastructure and
observability release that lays the foundation for the v0.44–v0.49 native
coverage expansion. It adds the row-vs-native parity harness in
`crates/pdl-parity-tests/`, the `selected_engine` regression-guard fixtures,
the refined typed `NativeUnsupportedReason` observability surface, and the
`--engine row-strict` CLI flag. It promotes no native coverage cells: the set
of pipelines that select the native backend under `--engine auto` is
identical to v0.42.

Version 0.42.0, tracked in `docs/V0_42_PLAN.md`, was an internal maintenance
release that split the oversized runtime, editor-services, and CLI render
modules without changing the language surface, diagnostics, output bytes, or
engine selection.

Version 0.40.0, tracked in `docs/V0_40_PLAN.md`, promoted selected remaining native gaps
where row/native semantics are explicit while keeping browser execution and
PDL-visible row semantics separate from Polars internals.

The native v0.40 implementation promotes compatible multi-key window ordering
for row-preserving mutate windows, typed native `lag`/`lead` defaults that lower
through supported native expressions, additional scalar functions
`contains`, `starts_with`, literal-pattern `replace`, `to_string`, and
`to_boolean`, and compile-time `col(...)` indirection through string literals
or string context defaults. Non-equi joins, incompatible-schema union
extensions, `pivot_longer`, `complete`, JSON Lines input, CSV/JSON Lines text
writers, browser/WASM execution, binding starts, and named outputs continue to
use the portable row runtime in automatic mode.

The native v0.39 implementation promotes row-preserving windows for
`percent_rank`, `cume_dist`, `lag`, `lead`, `first_value`, `last_value`, and
`rows between unbounded_preceding and current_row` aggregate frames where native
ordering, null handling, types, and frames match rows. It also promotes
single-key and composite-key equi-joins for `inner`, `left`, `right`, `full`,
`semi`, and `anti` at the existing native-safe binding boundary.

The native v0.38 implementation promotes row-preserving window expressions for
`row_number`, `rank`, `dense_rank`, and whole-partition `count`, `sum`, `mean`,
`min`, and `max`; and extends binding-backed single-key equi-joins to include
`right` and `full` joins.

The native v0.37 implementation promoted path-backed Arrow IPC file inputs,
Arrow IPC stdin and host-byte inputs, `to_number` and `if_else` scalar
lowering, binding-backed `inner`, `left`, `semi`, and `anti` single-key
equi-joins, and compatible-schema binding-backed `union` with optional
`distinct`. Windows, `pivot_longer`, `complete`, JSON Lines input, non-Arrow
stdin and byte-backed readers, binding starts, named outputs, and unsupported
join or union shapes continue to use the portable row runtime in automatic
mode.

Version 0.36.0 closes the native execution maturity slice tracked in
`docs/V0_36_PLAN.md`: it adds a first-class native coverage matrix, structured
plan/manifest observability for engine choice and fallback reasons, repeated
benchmark sampling with median/variance/memory fields, tracked PDL-to-Algraf
Arrow IPC smoke coverage, native `count_distinct` aggregate lowering, and
v0.36 release version stamps. The native v0.36 implementation supports
Polars-backed `mutate` for the supported simple expression subset, extends
native expression lowering for `filter`, `mutate`, and aggregate arguments,
and writes Parquet, Arrow IPC file, and Arrow IPC stream sinks directly from
native plans. CSV and JSON Lines output continue to use the row-format writer
path so text formatting remains stable.

Version 0.34.0 is a production native-pipeline release tracked in
`docs/V0_34_PLAN.md`. Its first implemented slice makes path-backed Arrow IPC
stream inputs eligible for native Polars execution when the rest of the
pipeline has native parity coverage. The implementation reads Arrow stream
files into the native dataframe path without converting through public row
`Value` objects, keeps byte-backed and stdin Arrow streams on the row runtime in
automatic mode before native scans are opened, and preserves the existing
browser/WASM rule that Polars, Parquet, and native Arrow dependencies MUST NOT
enter the `pdl-wasm` target graph.

Version 0.33.0 is a native execution performance follow-up release tracked in
`docs/V0_33_PLAN.md`. It makes automatic native execution classify unsupported
pipelines before opening native scans, adds native grouped aggregate coverage
for path-backed CSV and Parquet pipelines using `count`, `sum`, `mean`, `min`,
and `max`, keeps unsupported native cases on the row runtime in `auto` mode,
and extends `pdl-bench` reports with engine attribution and baseline
comparison. Browser/WASM builds continue to use the row runtime only and MUST
NOT enable Polars-backed native features.

Version 0.32.0 is a native execution performance foundation release tracked in
`docs/V0_32_PLAN.md`. It adds an opaque `pdl-data` data-plan facade beside the
existing `Table`, `Row`, and `Value` APIs; keeps concrete Polars, Arrow reader,
and Parquet reader types private to `pdl-data`; and keeps the portable row
runtime as the semantic reference and fallback. Native CLI execution may select
the Polars-backed backend for whole-pipeline path-backed plans containing
supported stages: `load` from a real path, `filter` with conservatively lowered
expressions, `select`, `drop`, `rename`, `sort`, `limit`, `distinct`, and
terminal `save` sinks. Unsupported stages, byte-backed inputs, named outputs,
bindings, and unsupported expressions fall back to the row runtime in automatic
mode. `pdl run --engine row|native|auto` makes backend selection observable for
debugging and parity tests. Arrow IPC stream stdout remains a first-class typed
handoff for both `--stdout-format arrow-stream` and terminal `save stdout format
"arrow-stream"` plans.

Version 0.31.0 is a benchmark infrastructure release tracked in
`docs/V0_31_PLAN.md`. It adds the `pdl-bench` crate, generated/downloaded
benchmark data lifecycle, tracked large workload programs, ignored
`bench/runs/<label>/report.csv` outputs, and tracked baseline snapshots under
`bench/baselines/`.

Version 0.30.0 is an npm publication-readiness release tracked in
`docs/V0_30_PLAN.md`. It keeps the v0.29 source language, execution semantics,
editor-service behavior, and browser JSON ABI stable while changing the
package-shaped browser integrations to publish generated `dist/` entrypoints:
CommonJS `dist/index.cjs`, ESM `dist/index.mjs`, TypeScript declarations, and
package-local browser assets such as `dist/pdl.wasm` for `pdl-wasm` and static
TextMate/language-configuration assets for `pdl-editor`.

Version 0.29.0 is a reactive context release tracked in
`docs/V0_29_PLAN.md`. It adds top-level `param` and `state` declarations,
`$name` parameter references, `@name` state references, runtime context maps for
native and browser execution, dynamic column-name resolution in column
positions, and `col(value)` for explicit expression-side column indirection.
Defaults keep documents runnable without host values; host overrides are typed
against declaration defaults and are supplied through runtime APIs rather than
source rewriting.

Version 0.28.0 is an editor semantic-token readability release tracked in
`docs/V0_28_PLAN.md`. It keeps the v0.27 source language, runtime, CLI, LSP
transport, and WASM execution semantics while adding parser-backed semantic
token categories for table binding declarations, table binding references,
column definitions, and column references. The token contract is exposed through
`pdl-editor-services`, the LSP semantic-token legend, the browser WASM JSON ABI,
the `pdl-wasm` TypeScript types, and the `pdl-editor` Monaco legend and default
theme.

Version 0.27.0 was a packaging and browser-editor integration release tracked in
`docs/V0_27_PLAN.md`. It keeps the v0.26 source language, runtime, CLI, LSP,
and WASM JSON ABI semantics while adding canonical editor assets under
`editors/assets/`, a package-shaped `pdl-wasm` browser runtime loader under
`packages/wasm/`, a package-shaped `pdl-editor` Monaco/React integration under
`editors/monaco/`, VS Code asset synchronization from the canonical assets, and
demo consumption of the shared editor package.

Version 0.26.0 was a breaking pre-1.0 syntax cleanup tracked in
`docs/V0_26_PLAN.md`. It removes context-sensitive quoted column references,
reserves double quotes for string/path literals, introduces backtick-escaped
column references, removes the `as`, `col(...)`, and `lit(...)` authoring
surfaces, and changes all column-producing stages to left-hand assignment.

The v0.26 implementation keeps the runtime, editor, LSP, native CLI
introspection, formatter, browser demo, browser WASM ABI, and window analytics
slices aligned with the new syntax. The README follows the Algraf project shape
with the PDL brand mark, public links, a concise example tour, Homebrew install
and upgrade commands, browser notes, and workspace layout. The browser demo
homepage shows the Homebrew CLI install commands directly below the live editor.
It retains the v0.24 browser WASM virtual text files for path-backed
named-output `save` sinks and the v0.23 story-table preparation surface:
decimal-place `round(value, digits)`, `count_distinct`, `pivot_longer`,
`complete`, and named top-level `output` declarations. It publishes GitHub
Release assets for the VS Code extension `.vsix` and standalone browser
`pdl.wasm` runtime, with both versioned filenames and `latest` aliases. It
retains recoverable syntax diagnostics for malformed filter, aggregate, sort,
missing-pipe, trailing-token, quoted-column, and legacy-`as` cases. It
implements the `pdl` CLI commands `run`, `check`, `fmt`, `schema`, `plan`,
`ast`, `ir`, `manifest`, `lsp`, and `version`; CSV, JSON Lines, Parquet, Arrow
IPC file, and Arrow IPC stream file loading and saving in native execution; CSV
and JSON Lines text stdout; binary Parquet, Arrow IPC file, and Arrow IPC stream
stdout where the host permits binary stdout; stdin format overrides and sniffing
for native execution; deterministic in-memory execution for `load`, `filter`,
`select`, `drop`, `rename`, `mutate`, `group_by`, `agg`, `sort`, `limit`,
`join`, `union`, `distinct`, `pivot_longer`, `complete`, and `save`; named
binding evaluation with lazy dependency resolution and cycle diagnostics; named
output evaluation in source order; the scalar functions `is_null`, `not_null`,
`coalesce`, `concat`, `lower`, `upper`, `trim`, `to_number`, `abs`, `round`, and
`if_else`; and the aggregate functions `count`, `count_distinct`, `sum`, `mean`,
`min`, and `max`. It implements window expressions with `row_number`, `rank`,
`dense_rank`, `percent_rank`, `cume_dist`, `lag`, `lead`, `first_value`,
`last_value`, `count`, `sum`, `mean`, `min`, and `max` over explicit
`partition_by`, `order_by`, and `rows between ... and ...` clauses in `mutate`.
It also
implements registered lettered diagnostic codes in `pdl-core`, a `codes::*`
registry, `related` spans and `help` diagnostic payload fields, diagnostic
catalog drift tests, a lossless lexer with trivia and EOF, a rowan CST with
typed AST views, a syntax-owned formatter boundary with multiline item-list and
window-expression formatting, driver I/O and phase-tagged
preparation reports, load-free driver source/stream/format plans, logical schema
surfaces, schema-cache and preview boundary types, semantic registries and IR,
execution planning from semantic IR plus driver facts, execution output emission
separated from planning, deterministic CLI JSON/text rendering for schema, plan,
AST, IR, and manifest inspection, crate-boundary drift tests, `pdl lsp` with
full-document sync, diagnostics, completion, driver-backed hover, formatting,
semantic tokens, document symbols, and same-document binding
definition/reference/rename including join/union binding references; and it
ships a thin VS Code client under
`editors/vscode/` plus browser-safe WASM ABI helpers that use in-memory driver
boundaries, including host-schema-backed checks, host-file-backed hover previews
for embedded editors, and a bounded host-file run facade for browser execution.
It also ships a React/Vite/Monaco demo under `demo/` with home, docs, and demos
routes; bundled CSV and JSON Lines fixtures; live PDL examples with host-file
input previews; editable host-supplied inputs; diagnostics, hover, completion,
formatting, semantic tokens, symbols, definition/reference, and rename from the
WASM editor-service ABI; CSV/JSON Lines stdout output from WASM execution; and
GitHub Actions workflows for the Rust test suite and GitHub Pages demo
deployment, plus GitHub Release asset publication for packaged editor and
browser outputs.

Version 0.29.0 does not yet implement configurable CSV dialect options, full
LSP code actions or cross-document navigation, Arrow IPC browser output,
output selectors or full multi-output browser controls.
Those features are tracked as deferred or planned work in successor release
plans after `docs/V0_26_PLAN.md`.

## 0. Document Contract

This document specifies PDL, a standalone Pipeline Data Language.

PDL produces deterministic tabular data that downstream consumers can read from
ordinary files or streams.

The intended file extension is `.pdl`.

The intended command-line executable is named `pdl`.

The first reference implementation target is a layered Rust workspace.

PDL is built around one core idea:

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount), avg_age = mean(customer_age)
  | sort total_revenue desc
  | limit 5
  | save "top_regions.csv"
```

The pipeline reads left to right.

Each stage consumes a table.

Each stage produces a table, a stream, or an explicit output artifact.

PDL is intentionally close to Unix pipes.

PDL declares how data becomes another table.

PDL programs should compose with other tools through ordinary artifacts:

```bash
pdl run prep.pdl --stdout-format arrow-stream > prepared.arrow
```

The PDL side of that contract is this:

PDL MUST be able to emit a deterministic Arrow IPC stream to stdout.

PDL MUST be able to emit deterministic CSV files.

PDL SHOULD be able to load Parquet files in the reference implementation.

PDL SHOULD be able to load and save Arrow IPC streams.

PDL MUST keep parser, semantic analysis, CLI behavior, LSP behavior, and WASM behavior aligned through shared crates.

The keyword `MUST` means required behavior.

The keyword `SHOULD` means recommended behavior.

The keyword `MAY` means optional behavior.

The keyword `MUST NOT` means prohibited behavior.

The keyword `implementation-defined` means behavior may vary, but the implementation must document the chosen behavior.

The keyword `diagnostic` means a machine-readable error or warning with source span information.

The keyword `resilient` means parsing or analysis continues after an error where practical.

The keyword `source span` means a byte range into the source document.

The keyword `pipeline` means an ordered chain of stages connected by `|`.

The keyword `stage` means one operation in a pipeline, such as `load`, `filter`, `group_by`, `agg`, `sort`, `limit`, or `save`.

The keyword `table` means an ordered, typed, rectangular relation with named columns.

The keyword `row` means one record in a table.

The keyword `window expression` means a row-preserving expression that evaluates
over a partition and order of rows. Version 0.26.0 specifies window expressions
in `mutate` assignments.

The keyword `column` means a named field with a static PDL type and nullability.

The keyword `schema` means the ordered set of columns, types, nullability, metadata, and optional key information for a table.

The keyword `stream` means a sequence of encoded table batches flowing through stdin, stdout, or an in-memory host boundary.

The keyword `source` means an input boundary such as a file path, stdin, or a named pipeline binding.

The keyword `sink` means an output boundary such as a file path or stdout.

The keyword `format` means a serialization format such as CSV, Parquet, Arrow IPC file, Arrow IPC stream, or JSON Lines.

The keyword `sniffing` means inspecting leading bytes or leading text to infer a format when a path extension is unavailable.

The keyword `binding` means a named top-level pipeline value introduced with `let`.

The keyword `CLI` means command-line interface.

The keyword `LSP` means Language Server Protocol.

The keyword `AST` means abstract syntax tree.

The keyword `CST` means concrete syntax tree.

The keyword `IR` means intermediate representation.

PDL uses lettered diagnostic code namespaces:

- `Exxxx` for author-facing errors
- `Wxxxx` for author-facing warnings
- `Hxxxx` for author-facing hints
- `Rxxxx` for implementation-oriented runtime/internal diagnostics

Diagnostic severity is independent of the code string.

The leading code letter SHOULD match serialized severity for author-facing
diagnostics. Internal `Rxxxx` diagnostics MUST still serialize explicit
severity.

Diagnostic codes emitted by an implementation MUST be reserved in this specification or in a versioned successor of this specification.

Every normative rule that requires an author-facing diagnostic SHOULD name the
stable diagnostic code in this specification before implementation.

Reserved diagnostic codes MAY be listed before implementation.

This specification distinguishes language behavior from reference implementation guidance.

Language constructs marked `MUST` define portable PDL.

Runtime facilities marked `SHOULD` define recommended behavior for the first full implementation.

Features marked `MAY` are extension points.

References to downstream consumers define interoperability context only.

A downstream consumer does not need to read PDL source.

A PDL implementation does not need to implement consumer-specific behavior.

## 1. Executive Summary

PDL is a domain-specific language for tabular data preparation.

It uses a concise pipe syntax instead of nested blocks.

The smallest useful program loads data, transforms it, and writes it:

```pdl
load "orders.csv"
  | filter amount > 0
  | select order_id, region, amount
  | save "orders_clean.csv"
```

The `load` stage creates the initial table.

The `filter` stage keeps rows.

The `select` stage keeps and orders columns.

The `save` stage writes the final table.

PDL supports aggregation:

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount), avg_age = mean(customer_age)
  | sort total_revenue desc
  | limit 5
  | save "top_regions.csv"
```

PDL supports named pipeline bindings for reuse and joins:

```pdl
let customers =
  load "customers.parquet"
  | select customer_id, segment

load "sales.parquet"
  | filter status == "completed"
  | join customers on customer_id kind left
  | group_by segment
  | agg revenue = sum(amount)
  | save "segment_revenue.csv"
```

Named bindings are still pipeline expressions.

They are not task blocks.

They are evaluated only when referenced by the main pipeline or by an output
pipeline.

PDL supports stdout for Unix composition:

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount)
```

When run with stdout enabled, the final table can be streamed:

```bash
pdl run sales.pdl --stdout-format arrow-stream
```

This makes PDL a natural producer for downstream consumers that read stdin.

The preferred typed handoff is Arrow IPC streaming.

CSV remains the required lowest-common-denominator file format.

PDL is deterministic.

PDL expressions are pure.

PDL has no loops in version 0.1.

PDL has no arbitrary user-defined functions in version 0.1.

PDL does not execute shell commands from source.

PDL does not fetch network resources by default.

The reference implementation should ship as one Rust binary named `pdl` with shared parser, analyzer, driver, execution, LSP, and WASM crates.

## 2. Design Goals

PDL MUST make tabular transformations explicit.

PDL MUST make input and output boundaries explicit.

PDL MUST support Unix-style composition through stdin and stdout.

PDL MUST support deterministic output for deterministic inputs.

PDL MUST support resilient parsing for incomplete source.

PDL MUST provide source spans for every diagnostic.

PDL MUST support static analysis without executing the full pipeline where practical.

PDL MUST support schema-aware editor features.

PDL MUST use shared parser and semantic analysis for CLI, LSP, and WASM.

PDL MUST support CSV output in version 0.1.

PDL MUST support Arrow IPC stream output in version 0.1.

PDL SHOULD support Parquet input in the reference implementation.

PDL SHOULD support Arrow IPC file input and Arrow IPC stream input.

PDL SHOULD support format sniffing for stdin.

PDL SHOULD provide explicit format overrides when sniffing is ambiguous.

PDL SHOULD be pleasant to type in terminals and editors.

PDL SHOULD be easy for LLMs to generate correctly.

PDL SHOULD keep syntax stable once examples are published.

PDL SHOULD provide deterministic JSON manifests for runs.

PDL SHOULD use stable column ordering.

PDL SHOULD avoid hidden global state.

PDL SHOULD isolate filesystem, process, and host I/O behind driver interfaces.

PDL MUST keep source language semantics independent of any particular dataframe engine.

The reference implementation MUST use Polars behind the `pdl-data` table
abstraction to drive dataframe transformations.

Polars implementation details MUST NOT leak into source syntax, diagnostics,
semantic IR, CLI JSON output, LSP payloads, or WASM host payloads.

PDL MAY support integrated host runners in later versions.

PDL MAY support package management in later versions.

PDL MAY support remote sources in later versions.

## 3. Non-Goals

PDL is not a general-purpose programming language.

PDL is not a charting language.

PDL is not an extension language for any downstream consumer.

PDL is not a scheduler.

PDL is not a replacement for SQL engines.

PDL is not a replacement for workflow orchestration systems such as Airflow, Dagster, or Prefect.

PDL does not initially support loops.

PDL does not initially support mutable variables.

PDL does not initially support user-defined scalar functions.

PDL does not initially support embedded Python, JavaScript, shell, or SQL scripts.

PDL does not initially support arbitrary network access.

PDL does not initially support streaming joins over unbounded streams.

PDL does not initially support distributed execution semantics.

PDL does not initially support transactions across multiple output files.

PDL does not initially mutate source files in place.

PDL does not initially guarantee zero-copy transfer across separate processes.

Arrow IPC streaming is near zero-copy and is the preferred cross-process handoff.

True in-process zero-copy between PDL and another runtime is a future
integrated-runner concern, not the PDL v0.1 language contract.

## 4. Core Concepts

### 4.1 Pipeline

A pipeline is an ordered expression connected by `|`.

The first stage MUST be `load` or a binding reference.

Every non-terminal transform stage consumes one table and produces one table.

Terminal output stages such as `save` produce an artifact and pass through the table unless otherwise specified.

This pass-through rule allows multiple saves:

```pdl
load "sales.parquet"
  | filter status == "completed"
  | save "completed.parquet"
  | group_by region
  | agg revenue = sum(amount)
  | save "regional_revenue.csv"
```

The pipeline result is the table produced by the final stage.

If the final stage is `save`, the result remains the saved table.

`pdl run` MAY stream the final table to stdout when requested.

### 4.2 Stage

A stage is one operation.

Stage names are lowercase keywords.

Stage arguments follow the stage name.

Examples:

```pdl
filter amount > 0
group_by region, channel
agg revenue = sum(amount)
sort revenue desc
limit 10
save "out.csv"
```

Unknown stage names MUST produce `E1201`.

Using a stage in an invalid position MUST produce `E1202`.

### 4.3 Table

A table is an ordered relation.

PDL preserves row order where a stage specifies preservation.

PDL produces deterministic row order where a stage changes ordering.

Table schemas are ordered.

Column names are case-sensitive.

Column order is significant for file output and editor display.

### 4.4 Column Reference

Simple column names are referenced as bare identifiers.

Column names that contain spaces, punctuation, or reserved words are referenced
with backticks.

Examples:

```pdl
filter status == "completed"
group_by region
agg total = sum(amount)
sort `sort` desc
```

Double-quoted tokens are string/path literals, not column references.

In expression context, an identifier followed immediately by `(` is a function
call. An identifier that is not followed by `(` is a column reference.

Examples of column-argument positions:

- `select a, b`
- `drop a`
- `group_by region`
- `sort amount desc`
- `sum(amount)`
- `join customers on customer_id`

If a v0.26 implementation sees a double-quoted token in a column position, it
SHOULD diagnose likely old syntax and suggest either a bare identifier or a
backtick-escaped column reference.

### 4.5 Literal

Literals include strings, numbers, booleans, and null.

String literals use double quotes.

Column references do not use double quotes.

Examples:

```pdl
filter status == "completed"
mutate label = concat(region, ": ", channel)
```

### 4.6 Binding

A binding names a pipeline expression.

Bindings use `let`.

Example:

```pdl
let completed =
  load "sales.parquet"
  | filter status == "completed"

completed
  | group_by region
  | agg revenue = sum(amount)
```

Binding names are identifiers.

Binding names live in file scope.

Binding names MUST be unique.

The final top-level pipeline is the main pipeline.

Bindings are evaluated lazily by dependency.

Bindings MUST NOT create dependency cycles.

### 4.7 Format

A format is an encoded representation of a table.

Required version 0.1 formats:

- `csv`
- `arrow-stream`

Recommended reference implementation formats:

- `parquet`
- `arrow-file`
- `jsonl`

Format names are lowercase strings or lowercase bare selectors in format-specific syntax.

CLI flags SHOULD use lowercase kebab-case names.

### 4.8 Artifact

An artifact is a file or stream produced by `save` or stdout output.

Artifacts MUST have deterministic bytes for deterministic inputs and options.

The runtime SHOULD record artifacts in a run manifest.

### 4.9 Diagnostic

Diagnostics are values.

Parser, analyzer, driver, data, exec, CLI, LSP, and WASM APIs SHOULD return outputs plus diagnostics.

Diagnostics MUST include severity, code, message, and source span when a source span is meaningful.

Runtime diagnostics without a source span MUST still include phase and context.

## 5. Syntax Overview

### 5.1 Minimal Pipeline

```pdl
load "sales.csv"
  | filter amount > 0
  | save "sales_clean.csv"
```

The pipeline loads a CSV file, filters rows, and writes a CSV file.

The output format is inferred from `.csv`.

### 5.2 Aggregation

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount), avg_age = mean(customer_age)
  | sort total_revenue desc
  | limit 5
  | save "top_regions.csv"
```

`group_by` establishes group keys.

`agg` consumes group state and emits one row per group.

`sort` orders rows.

`limit` keeps the first five rows.

### 5.3 Projection And Rename

```pdl
load "orders.csv"
  | select order_id = `Order ID`, order_date = `Order Date`, amount = `Amount`
  | save "orders_normalized.csv"
```

`select` may rename selected columns with left-hand assignment.

Column names with spaces use backtick-escaped column references.

### 5.4 Mutation

```pdl
load "orders.parquet"
  | mutate net_amount = gross_amount - discount
  | mutate is_large = net_amount >= 1000
  | save "orders_with_net.parquet"
```

`mutate` adds or replaces columns.

Assignments in one `mutate` stage are evaluated against the input schema in parallel.

Later stages see newly created columns.

The version 0.26.0 target language supports scalar row expressions and window
expressions in `mutate` assignments. New columns append in assignment order.
Replacing an existing column preserves that column's position. Duplicate
assignment targets in one stage MUST produce `E1207`.

### 5.5 Join

```pdl
let customers =
  load "customers.parquet"
  | select customer_id, segment

load "sales.parquet"
  | join customers on customer_id kind left
  | group_by segment
  | agg revenue = sum(amount)
  | save "segment_revenue.csv"
```

`join` references a binding or inline `load` source.

Version 0.1 SHOULD support joins against named bindings.

Inline join pipelines MAY be deferred.

### 5.6 Stdin And Stdout

```pdl
load stdin
  | filter status == "completed"
  | group_by region
  | agg revenue = sum(amount)
```

Run:

```bash
cat sales.arrow | pdl run prep.pdl --stdin-format arrow-stream --stdout-format arrow-stream
```

If `--stdin-format` is omitted, the driver SHOULD sniff the stream.

If `--stdout-format` is omitted and stdout is piped, the CLI SHOULD default to `arrow-stream`.

If stdout is a terminal, the CLI MAY default to a human-readable preview unless `--stdout-format` is supplied.

### 5.7 Stream Handoff

PDL source:

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg revenue = sum(amount)
```

Command:

```bash
pdl run prep.pdl --stdout-format arrow-stream > revenue.arrow
```

The `.pdl` file owns data preparation.

Downstream consumers own any later processing.

PDL MUST NOT mutate consumer-owned files.

### 5.8 Explicit Format Override

```pdl
load stdin format "csv"
  | filter status == "completed"
  | save stdout format "arrow-stream"
```

Explicit source syntax overrides sniffing.

CLI flags override runtime defaults.

If both source syntax and CLI flags specify incompatible formats for the same
stream, the CLI SHOULD produce `E1217` before reading.

## 6. Lexical Structure

### 6.1 Source Encoding

PDL source files MUST be valid UTF-8.

Source spans are byte offsets into UTF-8 source.

Source spans are half-open ranges `[start, end)`.

User-facing tools MAY display line and column positions.

LSP boundaries MUST convert byte offsets to UTF-16 positions.

### 6.2 Whitespace

Whitespace separates tokens.

Whitespace is otherwise insignificant.

Newlines do not terminate stages by themselves.

The pipe token `|` separates stages.

A leading `|` at the beginning of an indented line is the preferred formatting style for continuation stages.

### 6.3 Comments

Line comments begin with `//` and continue to end of line.

Block comments begin with `/*` and end with `*/`.

Block comments MAY nest.

Unterminated block comments MUST produce `E0003`.

Comments are trivia.

Comments MUST NOT affect semantics.

### 6.4 Identifiers

Identifiers begin with an ASCII letter or underscore.

Identifiers continue with ASCII letters, ASCII digits, or underscore.

Identifiers are case-sensitive.

Identifiers name bindings, bare stage selectors, stage options, and simple
column references where grammar permits columns.

Examples:

```text
customers
completed_sales
left
desc
```

### 6.5 Keywords

Reserved stage and declaration words in version 0.1 are:

- `load`
- `save`
- `filter`
- `select`
- `drop`
- `rename`
- `mutate`
- `group_by`
- `agg`
- `sort`
- `limit`
- `join`
- `union`
- `distinct`
- `pivot_longer`
- `complete`
- `let`
- `output`
- `param`
- `state`
- `on`
- `kind`
- `by_name`
- `names_to`
- `values_to`
- `fill`
- `format`
- `stdin`
- `stdout`
- `true`
- `false`
- `null`
- `and`
- `or`
- `not`
- `asc`
- `desc`

Reserved words MUST NOT be used as binding or output identifiers.

Column names may match reserved words only when referenced with backticks.

Window expression syntax uses additional clause words:

- `over`
- `partition_by`
- `order_by`
- `rows`
- `between`
- `unbounded_preceding`
- `current_row`
- `unbounded_following`
- `preceding`
- `following`

These words are reserved by the version 0.26.0 target language. `param` and
`state` are reserved by version 0.29.0 for reactive context declarations.

### 6.5.1 Reactive Context Declarations And References

Version 0.29.0 supports top-level context declarations before bindings,
outputs, or the main pipeline:

```pdl
param time_cutoff = 15
param active_fleet = "all"
state selected_zone = "Downtown"
```

`param` declares a host-controlled input. `state` declares a host-observed value
that may be updated by application interactions. Each declaration MUST include a
literal default (`string`, `number`, `bool`, or `null`) so the document remains
runnable when no host context value is supplied.

`$name` resolves a declared parameter and `@name` resolves a declared state.
Context references are value expressions. In column-name positions, a context
value MUST be a string and resolves to the active column name. In expression
positions where a context string should be treated as a column name rather than
a scalar value, authors MUST use `col(value)`, for example
`col($metric_column) > 500`.

### 6.6 String Tokens And Escaped Column References

Double-quoted tokens are string literals in expression context and path literals
in path context.

Double-quoted tokens MUST NOT be interpreted as column references in v0.26
syntax.

The escape sequences are:

- `\"`
- `\\`
- `\n`
- `\r`
- `\t`
- `\u{HEX}`

Unterminated string tokens MUST produce `E0002`.

Invalid escapes MUST produce `E0004`.

Backtick-delimited tokens are escaped column references.

They preserve all UTF-8 text between the delimiters except escaped backticks and
escaped backslashes.

Backticks are required when a column name contains spaces or punctuation, or
when a column name collides with a reserved word:

```pdl
select `Order ID`, `Gross Amount`
sort `sort` desc
```

Unterminated backtick column references MUST produce `E0002`.

Invalid backtick escapes MUST produce `E0004`.

### 6.7 Number Literals

PDL supports integer and decimal number literals.

Examples:

```text
0
42
-5
3.14
1e6
-2.5e-3
```

The lexer MAY emit `-` separately and let the parser construct unary negation.

### 6.8 Boolean And Null Literals

The boolean literals are `true` and `false`.

The null literal is `null`.

All are lowercase.

### 6.9 Punctuation

PDL uses:

- `|`
- `,`
- `=`
- `(`
- `)`
- `[`
- `]`
- `+`
- `-`
- `*`
- `/`
- `%`
- `==`
- `!=`
- `<`
- `<=`
- `>`
- `>=`

The parser MUST prefer the longest matching operator.

### 6.10 Token Spans

Every token MUST carry a source span.

Trivia SHOULD carry source spans in the CST.

Synthetic recovery tokens MUST be marked synthetic and MUST NOT claim source bytes.

## 7. Grammar

### 7.1 Program

```ebnf
Program       ::= Trivia* BindingDecl* OutputDecl* PipelineExpr? Trivia* EOF ;
BindingDecl   ::= "let" Ident "=" PipelineExpr ;
OutputDecl    ::= "output" Ident "=" PipelineExpr ;
PipelineExpr  ::= PipelineStart PipelineTail* ;
PipelineStart ::= LoadStage | Ident ;
PipelineTail  ::= "|" Stage ;
Stage         ::= TransformStage | SaveStage ;
```

A file contains zero or more `let` bindings followed by zero or more `output`
declarations followed by an optional main pipeline expression.

The main pipeline expression is the default run target when no `output`
declarations are present.

A file with one or more `output` declarations MUST NOT also contain a main
pipeline expression. Mixing both forms MUST produce `E1503`.

Output declaration names occupy the same top-level namespace as bindings.
Duplicate output names or output names that conflict with binding names MUST
produce `E1001`.

Output declarations evaluate in source order. An `output` declaration names the
materialized table result of its pipeline; it does not replace `save` as the
file or stream output boundary.

If a valid stage starts where a pipeline tail is expected but the `|` token is
missing, parsers MUST report `E0001` on the stage name and SHOULD recover as if
the pipe were present.

### 7.2 Load Stage

```ebnf
LoadStage     ::= "load" SourceRef FormatClause? ;
SourceRef     ::= StringToken | "stdin" | "-" ;
FormatClause  ::= "format" FormatName ;
FormatName    ::= StringToken | Ident ;
```

`load "path"` reads a file.

`load stdin` reads standard input.

`load -` is an alias for `load stdin`.

### 7.3 Save Stage

```ebnf
SaveStage     ::= "save" SinkRef FormatClause? SaveOptions? ;
SinkRef       ::= StringToken | "stdout" | "-" ;
SaveOptions   ::= SaveOption* ;
SaveOption    ::= "overwrite" BoolLiteral
                | "header" BoolLiteral ;
```

`save "path"` writes a file.

`save stdout` writes standard output.

`save -` is an alias for `save stdout`.

### 7.4 Transform Stages

```ebnf
TransformStage ::= FilterStage
                 | SelectStage
                 | DropStage
                 | RenameStage
                 | MutateStage
                 | GroupByStage
                 | AggStage
                 | SortStage
                 | LimitStage
                 | JoinStage
                 | UnionStage
                 | DistinctStage
                 | PivotLongerStage
                 | CompleteStage ;
```

### 7.5 Filter

```ebnf
FilterStage   ::= "filter" PredicateExpr ;
```

The predicate expression must evaluate to boolean or nullable boolean.

Rows are kept only when the predicate is true.

Adjacent operands in a `filter` predicate with no operator between them MUST
produce `E0001`.

A single `=` in a `filter` predicate is a malformed equality operator and MUST
produce `E0001`. Implementations SHOULD recover enough comparison shape to run
schema-backed column diagnostics for the left operand when a schema is
available.

### 7.6 Select, Drop, Rename

```ebnf
SelectStage   ::= "select" SelectItem ("," SelectItem)* ;
SelectItem    ::= ColumnRef | ColumnName "=" ColumnRef ;
DropStage     ::= "drop" ColumnRef ("," ColumnRef)* ;
RenameStage   ::= "rename" RenameItem ("," RenameItem)* ;
RenameItem    ::= ColumnName "=" ColumnRef ;
```

`select` keeps columns in listed order.

Assignment-form `select` items put the output column name on the left and the
source column on the right.

`drop` removes columns.

`rename` preserves order.

`rename` items put the new column name on the left and the existing source
column on the right.

### 7.7 Mutate

```ebnf
MutateStage   ::= "mutate" Assignment ("," Assignment)* ;
Assignment    ::= ColumnName "=" ValueExpr ;
```

The left side names a new or replaced column.

Assignments are evaluated in parallel against the input row. An assignment MUST
NOT reference a column created earlier in the same `mutate` stage.

### 7.8 Group And Aggregate

```ebnf
GroupByStage  ::= "group_by" ColumnRef ("," ColumnRef)* ;
AggStage      ::= "agg" AggItem ("," AggItem)* ;
AggItem       ::= ColumnName "=" AggCall ;
AggCall       ::= Ident "(" AggArgList? ")" ;
AggArgList    ::= ValueExpr ("," ValueExpr)* ;
```

`agg` consumes active group state.

If there is no active group state, `agg` aggregates the whole table into one row.

Aggregate items put the output column name on the left and the aggregate call on
the right.

### 7.9 Sort And Limit

```ebnf
SortStage     ::= "sort" SortItem ("," SortItem)* ;
SortItem      ::= ColumnRef SortDirection? NullsOrder? ;
SortDirection ::= "asc" | "desc" ;
NullsOrder    ::= "nulls_first" | "nulls_last" ;
LimitStage    ::= "limit" IntLiteral ;
```

`sort` is stable.

Invalid sort direction or null-order words MUST produce `E1210`.

Malformed sort items MUST produce `E1214`.

If a `sort` item without an explicit direction is followed by a newline and an
identifier that can begin the next top-level pipeline, parsers MUST treat the
`sort` item as complete. The following identifier MUST NOT be consumed as a
sort direction or null-order word.

`limit` keeps the first `n` rows in current order.

### 7.10 Join

```ebnf
JoinStage     ::= "join" JoinSource JoinOn JoinKind? ;
JoinSource    ::= Ident ;
JoinOn        ::= "on" JoinKey ("," JoinKey)* ;
JoinKey       ::= ColumnRef
                | "(" ColumnRef "," ColumnRef ")" ;
JoinKind      ::= "kind" JoinKindName ;
JoinKindName  ::= "inner" | "left" | "right" | "full" | "semi" | "anti" ;
```

`join customers on customer_id` joins the current table to the `customers` binding.

`on (left_id, right_id)` joins differently named keys.

`on customer_id, order_date` joins on a same-named composite key.

`on (sku, product_sku), (region, market)` joins on a differently named
composite key.

The default kind is `inner`.

### 7.11 Union And Distinct

```ebnf
UnionStage    ::= "union" Ident UnionOptions? ;
UnionOptions  ::= UnionOption* ;
UnionOption   ::= "by_name" BoolLiteral | "distinct" BoolLiteral ;
DistinctStage ::= "distinct" ColumnRefList? ;
ColumnRefList ::= ColumnRef ("," ColumnRef)* ;
```

`union` combines rows from the current table and a named binding.

Without options, `union` aligns columns by position.

`by_name true` aligns right rows by column name.

`distinct true` removes duplicate full rows after concatenation.

`distinct` removes duplicate rows.

Without a column list, `distinct` uses all columns as the duplicate key. With a
column list, `distinct` uses the listed key columns and retains the first row
for each key in current row order.

### 7.12 Pivot Longer And Complete

```ebnf
PivotLongerStage ::= "pivot_longer" ColumnRefList
                     "names_to" ColumnName "values_to" ColumnName ;
CompleteStage    ::= "complete" ColumnRefList CompleteFill? ;
CompleteFill     ::= "fill" CompleteFillItem ("," CompleteFillItem)* ;
CompleteFillItem ::= ColumnName "=" ValueExpr ;
```

`pivot_longer` reshapes selected source columns into name/value rows.

`complete` inserts missing key combinations from observed key-column values.

### 7.13 Expressions

```ebnf
ValueExpr     ::= OrExpr ;
OrExpr        ::= AndExpr (("or" | "||") AndExpr)* ;
AndExpr       ::= EqualityExpr (("and" | "&&") EqualityExpr)* ;
EqualityExpr  ::= CompareExpr (("==" | "!=") CompareExpr)* ;
CompareExpr   ::= AddExpr (("<" | "<=" | ">" | ">=") AddExpr)* ;
AddExpr       ::= MulExpr (("+" | "-") MulExpr)* ;
MulExpr       ::= UnaryExpr (("*" | "/" | "%") UnaryExpr)* ;
UnaryExpr     ::= ("not" | "!" | "-") UnaryExpr | PrimaryExpr ;
PrimaryExpr   ::= Literal
                | WindowExpr
                | CallExpr
                | ColumnRef
                | "(" ValueExpr ")" ;
WindowExpr      ::= CallExpr "over" "(" WindowSpec ")" ;
WindowSpec      ::= PartitionClause? OrderClause? WindowFrame? ;
PartitionClause ::= "partition_by" ColumnRef ("," ColumnRef)* ;
OrderClause     ::= "order_by" SortItem ("," SortItem)* ;
WindowFrame     ::= "rows" "between" FrameBound "and" FrameBound ;
FrameBound      ::= "unbounded_preceding"
                  | IntLiteral "preceding"
                  | "current_row"
                  | IntLiteral "following"
                  | "unbounded_following" ;
CallExpr        ::= Ident "(" ArgList? ")" ;
ArgList         ::= ValueExpr ("," ValueExpr)* ;
ColumnRef       ::= Ident | EscapedColumnRef ;
ColumnName      ::= Ident | EscapedColumnRef ;
```

Comparison chaining is not supported.

`a < b < c` MUST produce `E1408` or a type error with help suggesting
`a < b and b < c`.

Window expressions are specified in version 0.26.0 for `mutate`
assignments. Using a window expression outside `mutate`, nesting one window
expression inside another, or using a window function without `over (...)` MUST
produce `E1226`.

### 7.14 Error Recovery

The parser MUST recover from malformed pipelines.

Recovery points include:

- `|`
- newline followed by a stage keyword
- `let`
- `output`
- `save`
- EOF
- `,`
- `)`

The parser MUST NOT panic on malformed input.

The parser SHOULD produce partial AST nodes for LSP features.

## 8. Name Resolution And Scope

### 8.1 File Scope

Bindings live in file scope.

Binding names MUST be unique.

Output declaration names live in the same top-level file scope as bindings.

Output declaration names MUST be unique and MUST NOT conflict with binding
names.

The main pipeline can reference earlier bindings.

Bindings can reference earlier bindings.

Output pipelines can reference bindings.

Output declarations are not binding values and MUST NOT be referenced as
pipeline starts, join sources, or union sources unless a future selector feature
explicitly promotes that behavior.

Forward binding references MAY be allowed if the analyzer resolves the whole file before execution.

If forward references are allowed, cycles MUST still be rejected.

### 8.2 Pipeline Scope

Each stage sees the schema produced by the previous stage.

Column references resolve against that current schema.

Columns introduced by `mutate`, assignment-form `select`, `rename`, or `agg`
are available to later stages.

Dropped columns are unavailable to later stages.

### 8.3 Column Resolution

Column resolution is deterministic.

Unknown columns MUST produce `E1005`.

If schema is unknown during editor analysis, column references MAY be provisional.

Provisional column references SHOULD be labelled as such in hover or diagnostics.

### 8.4 Binding Resolution

Unknown bindings MUST produce `E1007`.

Binding cycles MUST produce `E1501`.

The diagnostic for a cycle SHOULD include the cycle path.

### 8.5 Shadowing

Bindings and stage keywords occupy different syntactic positions.

A binding may not use a reserved keyword.

Column names may match reserved keywords when referenced with backticks.

Column names may match binding names. Grammar position distinguishes pipeline
binding references from column references.

## 9. Type System

### 9.1 Primitive Types

PDL primitive types are:

- `string`
- `bool`
- `int`
- `number`
- `decimal`
- `date`
- `time`
- `datetime`
- `duration`
- `binary`
- `null`

Implementations MAY preserve richer source physical types internally.

The semantic type system MUST expose stable logical types to the analyzer and LSP.

### 9.2 Nullable Types

Every type may be nullable.

Source formats that support nulls map missing values to nullable types.

CSV parsing maps empty fields to null only when configured or inferred.

Expression operators MUST define null behavior.

`filter` keeps only rows whose predicate is true.

Rows whose predicate is false or null are dropped.

### 9.3 Numeric Types

`int` is a signed integer.

`number` is a floating point number.

`decimal` is exact decimal where supported by the data engine.

Arithmetic between `int` and `number` returns `number`.

Arithmetic involving decimal SHOULD preserve decimal when practical.

Division returns `number` unless an explicit integer division function is used.

### 9.4 String Types

String comparison is lexicographic by Unicode scalar value unless collation support is introduced.

Locale-sensitive collation is deferred.

String concatenation SHOULD use `concat(...)` rather than `+` in version 0.1.

### 9.5 Boolean Types

Boolean expressions use `and`, `or`, `not`, `&&`, `||`, and `!`.

Recommended nullable boolean behavior:

- `true and null` is `null`
- `false and null` is `false`
- `true or null` is `true`
- `false or null` is `null`
- `not null` is `null`

### 9.6 Temporal Types

Date and datetime parsing MUST be deterministic.

Timezone behavior MUST be documented.

The reference implementation SHOULD normalize timestamp-with-timezone values to UTC for comparison.

### 9.7 Type Inference

Explicit schema declarations are not part of the core v0.1 syntax.

PDL relies on source metadata, file schemas, and bounded inference.

Parquet and Arrow sources carry schemas.

CSV sources require inference or CLI/schema sidecars.

Inference MUST be deterministic.

Inference SHOULD be bounded and configurable.

LSP inference from sampled CSV data is provisional.

### 9.8 Type Diagnostics

Unknown types MUST produce `E1301`.

Incompatible operands MUST produce `E1302`.

Invalid assignment type MUST produce `E1303`.

Nullability violations SHOULD produce `E1308`.

Ambiguous inference SHOULD produce `E1309`.

## 10. Data Sources And Formats

### 10.1 Source Model

`load` accepts a path, `stdin`, or `-`.

Path sources infer format from extension unless a format clause or CLI override is supplied.

Stdin sources infer format by sniffing unless a format clause or CLI override is supplied.

Source reads MUST be explicit.

The runtime MUST NOT read files referenced only inside comments or strings unrelated to `load`.

### 10.2 CSV

CSV support is required.

CSV loading MUST support:

- UTF-8 input
- header rows
- comma delimiter by default

Configurable CSV delimiters, quote characters, and null tokens remain deferred
in version 0.26.0. A future release MAY promote them with source or CLI option
syntax, diagnostics, examples, and tests.

CSV output MUST be deterministic.

Default CSV output uses:

- UTF-8
- comma delimiter
- double quote quoting
- header row
- LF line endings
- stable column order

### 10.3 Parquet

Parquet support SHOULD be implemented in the reference implementation.

Parquet sources carry schema.

The version 0.15.0 native implementation supports Parquet schema reads, table
loads, file saves, and binary stdout where the host permits binary stdout.
Path inference recognizes `.parquet` and `.pq`.

Unsupported Parquet logical types MUST produce `E1808`.

Parquet reading SHOULD push down projection and filters where practical, but optimization MUST NOT change semantics.

The version 0.15.0 implementation does not yet push down projection or filters
and does not yet expose configurable row group sizing.

### 10.4 Arrow IPC File

Arrow IPC file format SHOULD be supported.

Arrow IPC files commonly start with magic bytes `ARROW1`.

The reader MUST validate Arrow metadata.

The writer SHOULD emit deterministic schema metadata.

The version 0.15.0 native implementation supports `arrow-file` schema reads,
table loads, file saves, and binary stdout where the host permits binary
stdout. Path inference recognizes `.arrow` and `.feather`, and `ipc` is accepted
as an alias for explicit format clauses.

### 10.5 Arrow IPC Stream

Arrow IPC stream format MUST be supported for stdout output.

Arrow IPC stream input SHOULD be supported for stdin.

Arrow streams begin with a continuation marker and schema message.

The v0.15.0 native implementation supports `arrow-stream` for `--stdout-format`,
`--stdin-format`, `load stdin`, `save stdout`, and explicit-format file
loads/saves. The v0.26.0 WASM browser run ABI continues to reject binary stdout
formats because its current stdout field is UTF-8 text.

The runtime SHOULD read and write record batches without unnecessary conversion.

The language does not require true zero-copy across OS processes.

### 10.6 JSON Lines

JSON Lines support MAY be implemented.

Each non-empty line must be a JSON object.

Nested objects are not flattened by default.

Schema inference for JSON Lines MUST be deterministic if implemented.

The version 0.15.0 native implementation supports `jsonl` and `ndjson` input,
output, stdin, stdout, and path inference. Schema inference uses the first
appearance order of object keys across non-empty rows. Output writes one object
per table row using table column order. Nested arrays and objects are represented
as compact JSON text cells rather than flattened or exposed as nested table
types.

### 10.7 Format Names

Canonical format names:

- `csv`
- `parquet`
- `arrow-file`
- `arrow-stream`
- `jsonl`

Aliases MAY be accepted:

- `arrow` as `arrow-stream` for stdin/stdout
- `ipc` as `arrow-file` for files
- `ndjson` as `jsonl`

Aliases MUST be documented.

### 10.8 Format Sniffing

When extension inference is unavailable, the driver SHOULD sniff leading bytes.

Sniffing SHOULD recognize:

- Arrow IPC file magic `ARROW1`
- Arrow IPC stream continuation marker and metadata size
- Parquet magic `PAR1`
- JSON or JSON Lines leading `{` or `[`
- UTF-8 text fallback as CSV

Sniffing MUST preserve the consumed bytes.

The driver MUST pass a reader containing the full stream to the selected parser.

If sniffing cannot distinguish formats, the driver MUST use deterministic
fallback order or produce `E1216`.

Recommended fallback order:

1. explicit format clause
2. CLI format override
3. file extension
4. magic-byte sniffing
5. text sniffing
6. CSV fallback

### 10.9 Explicit Format Override

Source syntax can specify format:

```pdl
load stdin format "arrow-stream"
load "input.data" format "parquet"
```

Sink syntax can specify format:

```pdl
save stdout format "arrow-stream"
save "output.data" format "csv"
```

CLI flags can specify stream formats:

```bash
pdl run prep.pdl --stdin-format csv --stdout-format arrow-stream
```

Explicit source syntax and CLI override conflicts MUST produce `E1217` unless
the CLI explicitly documents override precedence.

### 10.10 Source Security

Relative input paths resolve against the pipeline file directory by default.

Relative output paths resolve against the current working directory or configured output root by default.

The implementation MUST document path resolution.

Sandboxed runtimes MUST reject path traversal outside allowed roots.

Network URLs MUST be rejected by default.

## 11. Stage Semantics

### 11.1 Load

`load` creates the initial table.

`load` is valid only as a pipeline start.

`load` reads no data during pure parse.

Semantic analysis MAY read file metadata or bounded samples for schema inference.

Full data reads occur during run.

### 11.2 Filter

`filter predicate` keeps rows where predicate is true.

Predicate type must be boolean or nullable boolean.

`filter` preserves row order.

`filter` may refine nullability after checks such as `amount != null`.

### 11.3 Select

`select` keeps columns in listed order.

`select b = a` renames a selected column.

Unknown selected columns MUST produce `E1005`.

Duplicate output column names MUST produce `E1207`.

### 11.4 Drop

`drop` removes listed columns.

`drop` preserves order of remaining columns.

Dropping an unknown column MUST produce `E1005`.

Dropping all columns is legal but SHOULD produce `W2003`.

### 11.5 Rename

`rename new = old` renames columns.

Rename preserves column order.

Renaming to an existing column MUST produce `E1207` unless overwrite behavior is explicitly supported.

### 11.6 Mutate

`mutate name = expression` adds or replaces columns.

Assignments in one stage are parallel.

Later assignments in the same stage MUST NOT see earlier assignments unless a future sequential mode is introduced.

Replacing an existing column preserves its position.

New columns append in assignment order.

Duplicate assignment targets in one `mutate` stage MUST produce `E1207`.

The version 0.26.0 target language supports scalar row expressions and window
expressions in `mutate`.

### 11.7 Group By

`group_by` sets grouping state.

Grouping state is consumed by `agg`.

Grouping keys must exist.

Grouping keys remain first in aggregate output.

A pipeline ending with active group state and no `agg` SHOULD produce `W2001`.

### 11.8 Agg

`agg` aggregates rows.

With active group state, one output row is produced per group.

Without active group state, one output row is produced for the whole table.

Every aggregate item MUST use left-hand assignment.

A v0.25 aggregate alias form such as `sum(amount) as revenue` SHOULD produce a
targeted migration diagnostic and recover the output name for later
schema-backed diagnostics where practical.

Aggregate output column names MUST be unique.

Group output ordering MUST be deterministic.

Default ordering is ascending by group keys using PDL comparison rules.

### 11.9 Sort

`sort` orders rows by one or more columns.

Sort direction defaults to `asc`.

Sort MUST be stable.

Null order MUST be deterministic.

Default null order SHOULD be `nulls_last` for ascending and `nulls_first` for descending.

### 11.10 Limit

`limit n` keeps the first `n` rows.

`n` must be a non-negative integer.

`limit` preserves current order.

Using `limit` after a stage with unstable order SHOULD produce `W2004`.

### 11.11 Join

`join binding on key` joins the current table with a named binding.

`join binding on (left_key, right_key)` joins differently named keys.

`join binding on left_a, left_b` joins on a same-named composite key tuple.

`join binding on (left_a, right_a), (left_b, right_b)` joins on a differently
named composite key tuple.

Supported kinds:

- `inner`
- `left`
- `right`
- `full`
- `semi`
- `anti`

Default kind is `inner`.

Join key types MUST be compatible for every key pair. The CSV-backed reference
implementation checks observed non-null key value classes at execution time and
produces `E1208` when any left and right key classes are incompatible.

Composite-key joins compare the complete key tuple. If any key component on
either side is null, that row does not match.

For `inner`, `left`, `right`, and `full`, output columns are the left input
columns followed by right input non-key columns. Right join key columns are not
duplicated in output. For unmatched right rows in `right` and `full` joins, each
left key output column is populated from the corresponding right key value.

For `semi` and `anti`, output columns are the left input columns.

Duplicate non-key output column names MUST be resolved or diagnosed.

The default collision policy SHOULD suffix right-side columns with `_right`.

If a suffix creates another collision, the analyzer MUST produce `E1207` unless
an explicit suffix option is introduced.

Row ordering for `inner` and `left` preserves left input order and emits
matching right rows in right input order. `right` preserves right input order
and emits matching left rows in left input order. `full` emits matched and
unmatched left rows in left input order, then unmatched right rows sorted by the
join key tuple. `semi` and `anti` preserve left input order.

### 11.12 Union

`union binding` combines rows from the current table and a named binding.

Default behavior aligns columns by position. `by_name true` aligns right rows to
the left schema by column name.

Schemas MUST be compatible or produce `E1209`. Position-aligned union requires
the same number of columns. Name-aligned union requires the same column-name set.
The CSV-backed reference implementation also checks observed non-null value
classes at execution time.

`distinct true` removes duplicate rows.

Union output ordering preserves left rows followed by right rows. With
`distinct true`, duplicate full rows are removed after concatenation and the
first row in left-then-right order is retained.

### 11.13 Distinct

`distinct` removes duplicate rows using all columns.

`distinct a, b` removes duplicates using selected key columns.

The first row in current order is retained.

Output order follows retained row order.

Unknown distinct key columns MUST produce `E1005`.

### 11.14 Pivot Longer

`pivot_longer` converts selected source columns into name/value rows.

For each input row, the stage emits one output row for each selected source
column. Output order MUST be input row order followed by selected-column order.

Output columns are the input columns except the selected source columns, followed
by the `names_to` column and the `values_to` column.

The `names_to` value is the selected source column name. The `values_to` value is
the selected source column value from the input row.

The selected source column list MUST be non-empty. An empty or missing list MUST
produce `E1203`.

Unknown selected source columns MUST produce `E1005`.

Duplicate selected source columns MUST produce `E1205`.

`names_to` and `values_to` MUST be distinct and MUST NOT collide with retained
input columns. Collisions MUST produce `E1207`.

### 11.15 Complete

`complete` inserts rows for missing combinations of observed key-column values.

The key column list MUST be non-empty. A missing key list MUST produce `E1203`.

Key columns must exist. Unknown key columns MUST produce `E1005`.

Duplicate key columns MUST produce `E1205`.

For each key column, the stage records observed key values in first-appearance
order from the input rows. It then builds the Cartesian product of those observed
key-value lists. Existing rows are preserved. For each missing key tuple, one
new row is inserted.

Inserted rows populate key columns from the generated tuple. Unfilled non-key
columns are null. Fill assignments evaluate against the inserted base row and
write their target columns.

Fill target columns must exist. Unknown fill targets MUST produce `E1005`.

Duplicate fill targets MUST produce `E1205`.

A fill target MUST NOT be a key column. Violations MUST produce `E1207`.

If the input contains multiple existing rows for the same key tuple, `complete`
cannot choose a single existing row for that tuple and MUST produce `E1208`.

Output order is the Cartesian key tuple order using first-appearance key value
order. For existing tuples, the original row is emitted. For missing tuples, the
inserted row is emitted.

### 11.16 Window Expressions

Window expressions are row expressions that add or replace columns without
changing row count.

They are valid in `mutate` assignments:

```pdl
load "sales.parquet"
  | mutate region_revenue = sum(amount) over (partition_by region)
  | mutate region_rank = dense_rank() over (partition_by region order_by amount desc)
```

Running calculations use an explicit `rows between` frame:

```pdl
load "sales.csv"
  | mutate running_region_revenue =
      sum(amount) over (
        partition_by region
        order_by order_date
        rows between unbounded_preceding and current_row
      )
```

Offset calculations use `lag` or `lead` over an ordered partition:

```pdl
load "sales.csv"
  | mutate previous_amount =
      lag(amount) over (
        partition_by customer_id
        order_by order_date, order_id
      )
```

Window expressions MUST NOT use active `group_by` state.

`group_by` remains state for `agg` only; a window partition is always declared
explicitly with `partition_by`.

Window expressions preserve the current row order.

`partition_by` columns must exist.

`order_by` uses the same sort item syntax and null ordering rules as `sort`.

If `partition_by` is omitted, the whole input table is one partition.

If `order_by` is omitted, aggregate, offset, and value window functions operate
over the current partition order.

`rank`, `dense_rank`, `percent_rank`, and `cume_dist` require `order_by`.
`row_number` without `order_by` uses the current partition order.

Assignments in a `mutate` stage containing window expressions remain parallel:
one assignment MUST NOT see another assignment from the same stage.

The default frame for aggregate-style and value window functions is the whole
partition. Running calculations require an explicit frame. The frame
`rows between unbounded_preceding and current_row` MUST remain valid v0.26
syntax and means the first row in the partition through the current row.

Window execution MAY require materializing the current table or partition.

## 12. Expressions And Functions

### 12.1 Expression Contexts

PDL has row expression context, aggregate context, path context, and format context.

Row expressions can reference columns.

Aggregate expressions can reference aggregate functions and group keys.

Window expressions are a row-expression form valid in `mutate` assignments.
They do not introduce aggregate context, and they are not valid inside `agg`,
`filter`, `sort`, or other non-`mutate` expression positions in version 0.26.0.

Path context accepts string literals and future path functions.

Format context accepts canonical format names.

### 12.2 Column And Literal Resolution

Expression analysis MUST be deterministic.

Rules:

1. In expression and column positions, a bare identifier is a column reference
   unless it is followed by `(` and parsed as a function call.
2. A backtick-delimited token is always an escaped column reference.
3. A double-quoted token is always a string literal in expression context.
4. Path context accepts double-quoted path literals.
5. Format context accepts canonical format names.

Implementations SHOULD emit helpful diagnostics when source text appears to use
v0.25 quoted-column syntax or `as` aliases.

### 12.3 Scalar Functions

The version 0.40.0 target language supports these scalar functions in row
expressions:
- `is_null(value)`: returns true when the value is null.
- `not_null(value)`: returns true when the value is not null.
- `coalesce(value, ...)`: returns the first non-null value, or null if all
  arguments are null.
- `concat(value, ...)`: renders non-null arguments as text and concatenates
  them. Null arguments are skipped.
- `lower(value)`: renders a non-null value as text and lowercases it.
- `upper(value)`: renders a non-null value as text and uppercases it.
- `trim(value)`: renders a non-null value as text and trims leading and
  trailing whitespace.
- `contains(value, pattern)`: renders non-null arguments as text and returns
  true when `value` contains `pattern`. If either argument is null, the result
  is null.
- `starts_with(value, prefix)`: renders non-null arguments as text and returns
  true when `value` starts with `prefix`. If either argument is null, the result
  is null.
- `replace(value, pattern, replacement)`: renders non-null arguments as text and
  replaces every non-overlapping literal `pattern` occurrence in `value` with
  `replacement`. This is not a regex function. If any argument is null, the
  result is null.
- `to_string(value)`: renders a non-null value as text. Null returns null.
- `to_number(value)`: passes through numbers and parses text as a number.
  Empty, null, or unparseable values return null.
- `to_boolean(value)`: passes through booleans and parses trimmed text `true`
  or `false`. Null and unparseable values return null.
- `abs(value)`: returns the numeric absolute value.
- `round(value)`: rounds a numeric value to the nearest integer.
- `round(value, digits)`: rounds a numeric value to `digits` decimal places.
  `digits` MUST be a literal integer from `0` through `12`. Invalid digit
  values MUST produce `E1206`. Null numeric values propagate null. Results that
  would render as negative zero MUST normalize to `0`.
- `if_else(condition, when_true, when_false)`: returns `when_true` when the
  condition is true, `when_false` when it is false, and null when the condition
  is null.

Recommended future scalar functions:

- `ends_with(value, suffix)`
- `date(value)`
- `datetime(value)`
- `date_floor(value, unit)`
- `year(value)`
- `month(value)`
- `day(value)`

Unknown functions MUST produce `E1401`.

Invalid scalar function arity MUST produce `E1402`.

Function calls MUST be pure.

### 12.4 Aggregate Functions

Required aggregate functions:

- `count()`
- `count(column)`
- `count_distinct(column)`
- `sum(column)`
- `mean(column)`
- `min(column)`
- `max(column)`

Recommended aggregate functions:

- `median(column)`
- `stddev(column)`
- `first(column)`
- `last(column)`
- `n_distinct(column)`
- `any(predicate)`
- `all(predicate)`
- `null_count(column)`
- `null_rate(column)`

`count()` counts rows.

`count(column)` counts non-null values.

`count_distinct(column)` counts unique non-null values within the aggregate
group. Equality follows the implementation's deterministic value rendering for
distinct row keys.

`sum`, `mean`, `min`, and `max` ignore null values.

Aggregating an empty group returns null except for `count`, which returns zero.

### 12.5 Window Functions

Window function syntax is specified in version 0.26.0 for `mutate`
assignments.

Window functions use ordinary function-call syntax followed by an `over` clause.

Examples:

```pdl
load "orders.csv"
  | mutate customer_total = sum(amount) over (partition_by customer_id)
  | mutate running_total =
      sum(amount) over (
        partition_by customer_id
        order_by order_date asc
        rows between unbounded_preceding and current_row
      )
```

```pdl
load "orders.csv"
  | mutate rn =
      row_number() over (
        partition_by customer_id
        order_by order_date desc, order_id asc
      )
  | filter rn == 1
```

```pdl
load "orders.csv"
  | mutate previous_order_amount =
      lag(amount) over (
        partition_by customer_id
        order_by order_date asc, order_id asc
      )
```

Implemented ranking and distribution functions:

- `row_number()`
- `rank()`
- `dense_rank()`
- `percent_rank()`
- `cume_dist()`

Implemented offset and value functions:

- `lag(value)`
- `lag(value, offset)`
- `lag(value, offset, default)`
- `lead(value)`
- `lead(value, offset)`
- `lead(value, offset, default)`
- `first_value(value)`
- `last_value(value)`

Implemented aggregate-style window functions:

- `count()`
- `count(value)`
- `sum(value)`
- `mean(value)`
- `min(value)`
- `max(value)`

`lag(value)` and `lead(value)` use an offset of `1`.

`offset` must be a non-negative integer literal. If `lag` or `lead` moves
outside the partition, the function returns the `default` expression when
provided and `null` otherwise.

Aggregate-style and value window functions default to the whole partition, even
when `order_by` is present. Running calculations require an explicit frame:

```pdl
rows between unbounded_preceding and current_row
```

This frame includes the current row and every preceding row in the current
partition according to the window `order_by` order, or the current partition
order when no `order_by` is present.

Ranking, distribution, and offset functions ignore frames in version 0.26.0.

For `rank` and `dense_rank`, peer rows are rows with equal `order_by` values.

For `row_number`, rows with equal `order_by` values use the current stable row
order as the deterministic tie-breaker. Users SHOULD add explicit tie-breaker
columns when they need durable rankings independent of input order.

Invalid window specifications MUST produce a stable diagnostic such as `E1203`,
`E1204`, `E1205`, `E1206`, `E1214`, `E1226`, `E1401`, or `E1402`, depending on
the malformed clause.

### 12.6 Determinism

Version 0.1 functions MUST be deterministic.

Functions that read wall-clock time, random values, environment variables, filesystem metadata, or process state are not allowed in source expressions.

Such values must enter through CLI flags or future parameter mechanisms.

### 12.7 Expression Diagnostics

Invalid function arity MUST produce `E1402`.

Invalid function argument type MUST produce `E1403`.

Function not allowed in context MUST produce `E1404`.

Non-deterministic function not allowed MUST produce `E1405`.

Divide by zero detected statically MUST produce `E1407`.

Invalid window specifications MUST produce stable diagnostics; version 0.26.0
uses `E1203`, `E1204`, `E1205`, `E1206`, `E1214`, `E1226`, `E1401`, or
`E1402` depending on the malformed clause.

## 13. Row Ordering And Determinism

### 13.1 Row Order Rules

`load` establishes source order where the source has a natural order.

CSV source order is file row order.

Parquet source order is row group order followed by row order within row groups.

Arrow stream source order is batch order followed by row order.

`filter`, `select`, `drop`, `rename`, and `mutate` preserve row order.

`group_by` alone preserves row order as table state.

`agg` produces deterministic group-key order.

`sort` sets explicit order.

`limit` preserves current order.

`join` ordering depends on join kind and MUST be specified by the implementation.

Recommended left and inner join ordering preserves left input order.

Full joins SHOULD sort unmatched right rows by key after matched rows.

### 13.2 Deterministic Serialization

Output column order MUST be schema order.

CSV row order MUST be table order.

Arrow batch partitioning SHOULD be deterministic.

Parquet row group sizing SHOULD be deterministic when configurable.

JSON manifest object keys SHOULD be sorted or emitted in fixed order.

### 13.3 Floating Point

Floating point output formatting MUST be stable.

NaN and infinity handling MUST be documented.

The reference implementation SHOULD reject non-finite values for formats that cannot represent them clearly unless configured.

## 14. CLI Specification

### 14.1 Binary

The reference executable is `pdl`.

The CLI SHOULD be a single binary.

The CLI MUST support `run` and `check`.

The CLI SHOULD support `fmt`, `schema`, `plan`, `ast`, `ir`, `manifest`, `lsp`, and `version`.

### 14.2 pdl run

`pdl run file.pdl` parses, analyzes, plans, and executes the main pipeline or,
when the document declares named outputs, all output pipelines.

Recommended options:

- `--stdin-format <format>`
- `--stdout-format <format>`
- `--engine <auto|row|row-strict|native>`
- `--output <path>`
- `--manifest <path>`
- `--dry-run`
- `--strict`
- `--permissive`

If the pipeline has `save` stages, those stages write their artifacts.

If stdout output is requested, the final table is written to stdout.

Named output declarations MUST execute in source order.

If multiple named outputs would write distinct tables to one stdout stream, the
CLI MUST produce `E1607` instead of interleaving or concatenating data streams.

Operational logs MUST go to stderr so stdout remains a clean data stream.

Since version 0.32.0, the native CLI accepts `--engine auto`, `--engine row`,
and `--engine native` for `pdl run`. `auto` is the default and MAY choose the
native backend for a whole pipeline when the implementation can prove semantic
compatibility. Since version 0.33.0, `auto` MUST classify known-unsupported
native plans before opening native scans so row-only pipelines do not pay
failed-native execution overhead. `row` forces the portable row runtime.
`native` requires native backend execution and MUST report an ordinary PDL
diagnostic when the pipeline contains unsupported native operations.

Since version 0.43.0, the CLI also accepts `--engine row-strict` for
`pdl run`, `pdl plan`, and `pdl manifest`. `row-strict` plans and executes
exactly like `row` and additionally fails the run when the result reports any
backend other than the portable row runtime, proving the row engine still
handles the pipeline end-to-end with no silent native lowering. The
row-strict violation is a CLI-level error on stderr, not a new diagnostic
code; plan observability reports `requested_engine` `row-strict` with
`selected_engine` `row`.

### 14.3 pdl check

`pdl check file.pdl` parses and analyzes without executing the full pipeline.

It MUST report syntax and semantic diagnostics.

It SHOULD infer schemas from file metadata where cheap.

Human-readable CLI diagnostics SHOULD use rustc-style compact source rendering:
a `severity[code]: message` header, a `--> file:line:column` location line, a
guttered source line, and a caret underline for the primary span.

When diagnostic help or related spans are present, human-readable CLI
diagnostics SHOULD render them with rustc-style `= help:` and `= note:` lines.

Human-readable CLI diagnostics SHOULD color the severity label and primary caret
underline when stderr is a terminal.

The CLI MUST suppress diagnostic color when stderr is not a terminal or when the
`NO_COLOR` environment variable is set.

Diagnostic color MUST NOT affect JSON diagnostic payloads or stdout data.

Human-readable diagnostic rendering belongs to `pdl-cli`. `pdl-core` owns
diagnostic values and source-position helpers, but MUST NOT expose
terminal-specific diagnostic formatting.

It SHOULD avoid reading full data files by default.

It MUST exit non-zero on errors.

### 14.4 pdl schema

`pdl schema file.pdl` prints inferred schema for the main pipeline, or the last
declared output when a document contains named outputs and no selector is
provided.

`pdl schema file.pdl --binding name` prints schema for a binding.

`pdl schema file.pdl --json` prints deterministic JSON.

The version 0.26.0 implementation emits column names, unknown logical types,
nullability, stage traces, named output schemas, and diagnostics in JSON mode.

`--binding name` MUST inspect the requested binding without changing normal
`check` and `run` lazy-binding behavior.

### 14.5 pdl plan

`pdl plan file.pdl` prints the execution plan.

It MUST not write output artifacts.

It SHOULD show source reads, transform stages, format decisions, and sinks.

`pdl plan file.pdl --json` prints deterministic JSON.

The version 0.26.0 implementation accepts `--stdin-format <format>` and
`--stdout-format <format>` so stream choices are reflected in the plan. It MUST
NOT read stdin or execute transforms while planning. Plans for named-output
documents MUST include output boundaries in declaration order.

### 14.6 pdl fmt

`pdl fmt file.pdl` formats source.

`pdl fmt --check file.pdl` checks formatting without writing.

The formatter MUST preserve semantics.

The version 0.26.0 implementation rewrites files in place in the stable
leading-pipe style when formatting is available. It keeps short item lists
inline, expands long item-list stages, and expands top-level window assignments
in `mutate`. It returns a non-zero exit code without writing when parse errors
are present or when comments make safe rewriting unavailable.

### 14.7 pdl ast

`pdl ast file.pdl` prints deterministic JSON for the parsed program shape.

It MUST NOT execute data pipelines or read table data.

The version 0.26.0 implementation exits non-zero on parse errors. When parsing
succeeds, its JSON payload includes the parsed program, output declarations, and
parse diagnostics.

Since version 0.39.0, AST JSON preserves the existing join `same` and `pair`
shapes for existing `on key` and `on (left, right)` source. Composite join
syntax emits an additive `composite` join-on shape with ordered key pairs.

### 14.8 pdl ir

`pdl ir file.pdl` prints deterministic JSON for the semantic IR.

It MUST NOT execute data pipelines or write output artifacts.

The version 0.26.0 implementation exits non-zero when syntax, schema, or
semantic errors prevent IR construction. Successful IR JSON includes output
declarations when present.

Since version 0.39.0, join IR JSON continues to emit `left_key` and `right_key`
for compatibility. Composite joins additionally emit a `keys` array only when a
join has more than one key pair.

### 14.9 pdl manifest

`pdl manifest file.pdl` prints deterministic manifest JSON for a dry-run plan.

It MUST NOT execute transforms or write output artifacts.

The version 0.26.0 implementation accepts `--stdin-format <format>` and
`--stdout-format <format>`, includes source, driver, stream, execution-plan,
final-schema, output schemas, diagnostics, and Arrow-stdout stream hint fields, and exits
non-zero when planning fails.

### 14.10 pdl lsp

`pdl lsp` runs the language server over standard input and standard output.

The LSP backend MUST share parser and analyzer code with CLI.

### 14.11 Exit Codes

Exit code `0` means success.

Exit code `1` means diagnostics or runtime failure.

Exit code `2` means CLI usage error.

Additional exit codes MAY be defined.

### 14.12 Stdout Discipline

When stdout is used for data, all human-readable logs MUST go to stderr.

Diagnostics in human-readable form MUST go to stderr.

JSON diagnostics MAY go to stdout only for commands whose output is diagnostics rather than data, such as `check --json`.

Introspection commands such as `schema`, `plan`, `ast`, `ir`, and `manifest`
own their stdout payloads. Human-readable diagnostics for those commands still
MUST go to stderr.

## 15. Driver, Planning, And Execution

### 15.1 Driver Responsibilities

The driver prepares a pipeline for analysis and execution.

The driver owns path resolution, format detection, stream source descriptors, schema loading orchestration, and runtime I/O boundaries.

The driver MUST not depend on CLI, LSP, or WASM crates.

### 15.2 Pipeline Plan

The planner turns analyzed IR into an execution plan.

The plan records:

- binding dependencies
- source reads
- format decisions
- transform stages
- output sinks
- stdout behavior
- manifest behavior

Plans MUST be deterministic.

### 15.3 Lazy Bindings

Bindings are evaluated only if referenced.

If multiple stages reference the same binding, the runtime MAY cache it.

Cache reuse MUST be conservative.

If the runtime cannot prove a cached table is valid, it MUST recompute.

### 15.4 Streaming Execution

PDL SHOULD stream where semantics permit.

### 15.5 Data Backend Facade

The reference implementation keeps the public row API available through
`pdl-data` while adding an opaque data-plan facade for execution engines.

`pdl-data` MUST expose backend-neutral data source, sink, plan, and expression
types. These types MAY report whether the selected backend is portable rows or
native Polars, but public APIs above `pdl-data` MUST NOT expose Polars
dataframes, lazy frames, expressions, Arrow reader internals, or Parquet reader
internals.

The portable row backend is the reference behavior. Native execution is enabled
only for whole-pipeline plans whose inputs, expressions, stages, and output
sinks have parity coverage. If parity is uncertain, automatic execution MUST
use the row backend.

The first native fast path in version 0.32.0 is limited to path-backed plans
with supported stages. Since version 0.33.0, supported native stages include
grouped `agg` for `count`, `sum`, `mean`, `min`, and `max` over simple column
references on path-backed CSV and Parquet inputs. Since version 0.34.0,
path-backed Arrow IPC stream inputs are also eligible for native execution when
the rest of the pipeline has native parity coverage.

Since version 0.35.0, the native subset also supports row-preserving `mutate`
assignments and aggregate arguments when each expression can be lowered to the
shared native expression subset. That subset includes column references,
numeric, string, boolean, and null literals, arithmetic, comparison operators,
boolean `and`, `or`, and `not`, and the scalar functions `is_null`,
`not_null`, `coalesce`, `concat`, `lower`, `upper`, `trim`, `abs`, and `round`.
Supported native `mutate` assignments are applied as one parallel projection:
later assignments in the same `mutate` stage do not see earlier assignments,
replacements keep existing column positions, and new columns append in
assignment order.

Since version 0.36.0, native aggregate coverage includes `count_distinct(expr)`
over the supported native expression subset. Null values are excluded from the
distinct count to match row runtime semantics.

Since version 0.37.0, the native subset also supports `to_number(expr)` and
`if_else(condition, when_true, when_false)` over the supported native expression
subset, path-backed Arrow IPC file inputs, Arrow IPC file/stream stdin bytes,
Arrow IPC file/stream host bytes, and multi-input native pipelines where the
main input uses a supported `join` or `union` against a native-safe
binding-backed input. Native `if_else` preserves null-condition behavior for
typed native branch outputs; branch result types must remain compatible with
the native column model. Native `join` coverage is limited to `inner`, `left`,
`semi`, and `anti` single-key equi-joins. Null join keys do not match,
duplicate right non-key output names use the row runtime's `_right` suffix rule
where right columns are emitted, and output order must match row runtime order
for the promoted slice. Native `union` coverage is limited to compatible schemas
by name or by position, with optional `distinct` when the existing native
`distinct` semantics apply. Arrow IPC file/stream byte inputs may be read into a
native dataframe before lazy transforms continue; PDL does not expose Arrow
reader internals.

Since version 0.38.0, the native subset also supports `right` and `full`
single-key equi-joins for path-backed main inputs joined to native-safe
binding-backed inputs. Null join keys do not match, output columns follow the
row runtime's coalesced-key and `_right` suffix contract, right joins preserve
right input order, and full joins preserve left rows before appending unmatched
right rows sorted by key. Native row-preserving window coverage includes
`row_number`, `rank`, `dense_rank`, and whole-partition `count`, `sum`, `mean`,
`min`, and `max` over supported native expressions with at most one order key.
The native implementation maps window results back to original dataframe rows;
it does not use exploding or list-joining window mappings that would change PDL
row order or shape.

Since version 0.39.0, the native subset also supports composite-key equi-joins
for `inner`, `left`, `right`, `full`, `semi`, and `anti` joins at the same
path-backed main input and native-safe binding-backed right input boundary.
Composite native joins preserve the row runtime's null-key non-match rule,
right non-key suffixing, coalesced key output, and deterministic output order.

Native row-preserving window coverage also includes `percent_rank`,
`cume_dist`, `lag`, `lead`, `first_value`, `last_value`, and
`rows between unbounded_preceding and current_row` aggregate frames when each
argument lowers through the supported native expression subset. Native
`lag`/`lead` require exactly one order key, a non-negative integer literal
offset, and an omitted or `null` default. Native ranking, distribution, and
offset windows currently require at most one order key so per-key direction,
null placement, and tie behavior remain exactly aligned with the row runtime.

Since version 0.40.0, the native subset also supports compatible multi-key
window ordering for row-preserving mutate windows. The native executor adds a
hidden row index, physically pre-sorts by partition keys and the composite
window order, evaluates windows over that ordered partition, restores original
row order, then drops the hidden index. The promoted multi-key subset requires a
single compatible composite order group per mutate stage; mixed multi-key order
groups remain row-only in automatic mode or report `E1211` in forced native
mode. Multi-key peer rows for `rank`, `dense_rank`, `percent_rank`, and
`cume_dist` compare every `order_by` key with null-aware equality.

Since version 0.40.0, native `lag` and `lead` support non-null default
expressions when the value expression, offset, default, and window spec all
lower through the native subset and the resulting typed native branch output is
compatible. Incompatible native branch dtypes may still fall back to rows in
automatic mode or fail forced native with `E1211`.

Since version 0.40.0, the native scalar subset also supports `contains(value,
pattern)`, `starts_with(value, prefix)`, literal-pattern
`replace(value, pattern, replacement)`, `to_string(value)`, and
`to_boolean(value)` over supported native expressions. Native `replace` is
limited to literal or context-literal pattern and replacement arguments because
the backend does not support dynamic per-row replace patterns with the required
row semantics.

Since version 0.40.0, `col(...)` with a string literal or string context default
is eligible for native planning as a static column reference. Data-dependent
`col(...)` remains row-only.

Unsupported aggregate functions, non-Arrow byte-backed input, non-Arrow stdin,
binding starts, named outputs, multi-output execution, non-equi joins,
incompatible-schema union extensions, `pivot_longer`, `complete`, JSON Lines
input, CSV/JSON Lines text writers, unsupported bounded-frame windows,
incompatible multi-key window order groups, data-dependent dynamic `col(...)`,
uncertain coercions, and other unsupported expressions fall back to
rows in automatic mode before native scans are opened when they are known to be
unsupported. Forced native mode reports an `E1211` diagnostic with a stable
unsupported native reason category instead of silently falling back.

Since version 0.43.0, the unsupported-native reason surface is typed.
`NativeUnsupportedReason` in `pdl-exec` replaces the v0.40–v0.42 coarse
free-form fallback categories with coverage-boundary variants:
`no-runnable-main`, `input-format`, `scalar-function`,
`scalar-function-arity`, `aggregate-function`, `aggregate-arity`,
`window-expression`, `data-dependent-col-indirection`,
`data-dependent-replace-pattern`, `unsupported-numeric-coercion` (v0.47
reserve), `union-null-padding` (v0.41 reserve), `non-equi-join` (v0.41
reserve), `binding-start-not-eligible`, `named-output-mixed-engines`,
`non-terminal-save-fanout`, `stdin-bytes-backed-scan`,
`host-bytes-backed-scan`, `native-sink-writer`, `row-only-stage`,
`driver-facts`, and the non-execution documentation boundaries
`wasm-target-graph` and `editor-service`. These are internal observability
values, not diagnostic codes. `pdl plan` text output names the variant, and
`pdl plan --json` and `pdl manifest` serialize it under
`execution.observability.fallback_reason`. Every runnable pipeline that is
not natively eligible MUST carry a variant. Row-dependent `col(...)` reports
`data-dependent-col-indirection`, and row-dependent `replace` patterns or
replacements report `data-dependent-replace-pattern`.

Version 0.43.0 also ships the row-vs-native parity harness
(`cargo test -p pdl-parity-tests parity_examples`) and the silent-demotion
canary (`cargo test -p pdl-parity-tests selected_engine_fixtures`). The
harness runs every example in `examples/` through `pdl run` on the row,
row-strict, auto, and (for examples pinned native) forced native engines
with stdin fixtures supplied per example, then diffs stdout payloads and
saved or named-output files against the row engine. The row engine is the
parity spec: CSV and JSON Lines payloads MUST match byte-for-byte. Arrow IPC
file, Arrow IPC stream, and Parquet payloads are compared as decoded tables
because the native direct writers emit semantically equal but not
byte-identical encodings; unifying those bytes is v0.44 native sink writer
work. Each example carries a `selected_engine` fixture under
`crates/pdl-parity-tests/fixtures/selected_engine/`, and the canary MUST
fail when an example flips engine under `--engine auto` without a fixture
update that travels in the same commit as a plan promotion entry.

Browser/WASM builds MUST keep the native Polars feature set disabled. The WASM
runtime MUST NOT enable `native-formats`, `polars-engine`, or any dependency
path that pulls Polars, Arrow, or Parquet into the wasm target dependency graph.

`filter`, `select`, `drop`, `rename`, and simple `mutate` can stream.

`group_by` plus `agg`, `sort`, `join`, `distinct`, window expressions, and
some `union` modes may require materialization.

Since version 0.36.0, `pdl plan --json` and `pdl manifest` include a stable
execution observability object with requested engine, selected engine, eligible
engine, native eligibility, fallback reason, source boundary, input format,
output format, sink strategy, blocking stages, public row-materialization
status, and required source columns where the planner can compute them. Human
plan output also includes the same facts. Observability MUST NOT write to binary
stdout during `run`; it is exposed through plan/manifest JSON, text plan output,
stderr diagnostics, or benchmark sidecar reports.

Version 0.40.0 defines the native coverage matrix in
`docs/PDL_NATIVE_COVERAGE.csv` and documents it in
`docs/PDL_NATIVE_COVERAGE.md`. Matrix statuses are limited to `native parity`,
`native partial`, `row-only by design`, `planned native`, `unsupported`, and
`deferred`.

The plan SHOULD identify blocking stages.

### 15.5 Failure Semantics

Static errors stop execution.

Runtime source errors stop dependent stages.

Write errors fail the run.

In permissive mode, row-level parse errors MAY be collected while execution continues.

Strict mode MUST fail on row-level parse errors.

### 15.6 Manifests

The runtime SHOULD emit a run manifest when requested.

The version 0.36.0 native CLI implements `pdl manifest file.pdl` as a
deterministic dry-run manifest inspection command. It plans but does not execute
the pipeline, and it does not write output artifacts.

Manifest fields SHOULD include:

- PDL source path
- implementation version
- input sources
- detected formats
- output artifacts
- final schema
- named output schemas
- row counts where known
- content hashes where computed
- diagnostics
- stream interop hints when stdout format is Arrow IPC
- execution observability for selected engine, fallback reason, sink strategy,
  blocking stages, row-materialization status, and required source columns

Manifest JSON MUST be deterministic.

### 15.7 Benchmark Reports

The `pdl-bench` crate is the repository-local benchmark harness.

Since version 0.36.0, `pdl-bench run` SHOULD support repeated measured samples,
warmups, randomized workload order, and optional cool-down between samples.
Reports SHOULD include min, median, p90, max, standard deviation, measured
sample count, warmup count, failed sample count, unsupported-native sample
count, output rows, output bytes, selected engine, eligible engine, fallback
reason, sink strategy, row-materialization status, required source columns,
system metadata, Rust version, build profile, git ref, dirty flag, feature
flags, and peak RSS where the developer platform exposes it.

`pdl-bench compare` SHOULD compare medians when present and fall back to
single-run elapsed time for older reports. Regression gates SHOULD be
configurable by both absolute milliseconds and relative percentage.

## 16. Stream Interoperability

### 16.1 Interop Principle

PDL prepares tables for downstream consumers.

The preferred interop boundary is Arrow IPC streaming over stdout/stdin.

CSV files are the portable fallback.

### 16.2 Unix Arrow Streaming

PDL SHOULD support:

```bash
pdl run prep.pdl --stdout-format arrow-stream
```

Workflows can pipe this stream into any consumer that understands Arrow IPC:

```bash
pdl run prep.pdl --stdout-format arrow-stream | consumer --stdin-format arrow-stream
```

PDL's responsibility is to produce a valid Arrow IPC stream.

The native backend SHOULD write Arrow IPC stream stdout directly through a
writer-oriented data sink when the active native plan can do so without
materializing a public row table. Diagnostics and logs MUST continue to use
stderr so stdout remains stream bytes only.

The v0.36 native writer path writes Parquet and Arrow IPC file sinks directly
from native plans. CSV and JSON Lines output remain on the public row-format
writer path because their exact text formatting is PDL-visible behavior.

The consumer's responsibility is to consume stdin if it supports that mode.

This PDL specification does not require downstream consumers to implement new
flags.

### 16.3 Stdin Format Sniffing For Consumers

PDL recommends that consumers reading unknown stdin support sniffing plus explicit override.

For PDL itself, this is normative for `load stdin`.

For downstream consumers, this is interop guidance only.

### 16.4 File-Based Handoff

PDL can materialize a file:

```pdl
load "sales.parquet"
  | group_by region
  | agg revenue = sum(amount)
  | save "build/revenue.csv"
```

Downstream consumers can reference that file:

```text
consumer build/revenue.csv
```

File-based handoff is slower than Arrow streaming but simpler to inspect and cache.

### 16.5 Browser Handoff

In browser hosts, PDL WASM SHOULD be able to return Arrow bytes as a `Uint8Array` through the host ABI.

A host MAY pass those bytes to another WASM runtime if one is loaded.

PDL WASM MUST NOT invoke native external processes.

PDL WASM MUST NOT assume another WASM runtime is present.

### 16.6 Integrated Runner

A future integrated runner MAY link PDL and another runtime in one Rust process.

Such a runner MAY pass table memory directly without IPC.

This is outside PDL v0.1 core.

PDL source syntax MUST NOT depend on the integrated runner.

## 17. LSP And Editor Services

### 17.1 LSP Goals

The PDL LSP MUST use the same parser as CLI.

The PDL LSP MUST use the same analyzer as CLI.

The PDL LSP MUST recover from incomplete source.

The PDL LSP MUST provide diagnostics.

The PDL LSP SHOULD provide completion, hover, formatting, semantic tokens, code actions, go to definition, references, rename, and document symbols.

The current `0.43.0` LSP implementation provides diagnostics,
completion, driver-backed hover, formatting, parser-backed semantic tokens,
document symbols, schema-aware output declarations, and same-document binding
go-to-definition, references, and rename. Code actions, output selectors, and
cross-document navigation remain deferred.

The current formatter withholds edits for documents containing comments because
comment attachment is not yet source-preserving. The lexer and CST preserve
comment trivia, but the formatter does not yet have stable placement rules for
rewriting commented documents.

### 17.2 Completion

Completion SHOULD support:

- stage names after `|`
- declaration names at top level
- binding names at pipeline starts and join sources
- column names in column positions
- aggregate function names
- scalar function names
- format names
- join kinds
- sort directions

Column completions MUST be schema-aware where schema is known.

### 17.3 Hover

Hover SHOULD show:

- stage documentation
- column type and nullability
- binding schema
- aggregate function signatures
- detected source format
- diagnostic explanations

For path-backed CSV loads, hover on the load path SHOULD show a bounded Markdown
preview derived from shared driver/editor-service I/O. The preview SHOULD include
the detected format, sampled row count, column names, derived logical types,
nullability, sample values, and a small sample-row table.

For schema-known CSV columns, hover SHOULD show the current column name, derived
logical type, nullability, and a small set of sample values when host data is
available.

Native LSP hover and browser Monaco/WASM hover MUST use the shared Rust
editor-service implementation. VS Code clients, Monaco hosts, and other editor
adapters MUST NOT implement independent PDL parsing, semantic analysis, or CSV
type inference.

### 17.4 Semantic Tokens

Editor semantic tokens MUST be produced by the shared Rust editor-service
implementation. LSP, WASM, Monaco, VS Code clients, Studio, and other browser
hosts MUST NOT reimplement PDL parsing or semantic token classification in
TypeScript or other host-side adapter code.

The editor-service token contract MUST include these token kinds:

- `Keyword`
- `Function`
- `Variable`
- `String`
- `Number`
- `Operator`
- `BindingDeclaration`
- `BindingReference`
- `ColumnDefinition`
- `ColumnReference`

`BindingDeclaration` MUST classify the name introduced by a top-level `let`
binding. `BindingReference` MUST classify table binding references at pipeline
starts and in stages that consume named table bindings, such as `join` and
`union`.

`ColumnDefinition` MUST classify column names introduced or rewritten by output
positions, including `mutate` assignment targets, aggregate aliases, select
aliases, rename destination names, `pivot_longer` `names_to` and `values_to`
names, and `complete fill` targets.

`ColumnReference` MUST classify parsed column read positions, including filter
expressions, select source columns, drop columns, rename source columns, mutate
expressions, aggregate arguments, group keys, sort keys, distinct keys, join
keys, pivot source columns, complete keys, complete fill expressions, and
window `partition_by` and `order_by` columns.

The LSP semantic-token legend MUST preserve the existing standard token order
for `keyword`, `function`, `variable`, `string`, `number`, and `operator`, then
append the PDL-specific token types `pdlBindingDeclaration`,
`pdlBindingReference`, `pdlColumnDefinition`, and `pdlColumnReference`.

The WASM editor-service JSON ABI MUST serialize the editor-service token kind
names exactly as the Rust `SemanticTokenKind` variants. The `pdl-wasm`
TypeScript types and `pdl-editor` Monaco provider MUST expose the same names.
The Monaco legend MUST map those names to the LSP-style custom token type names
listed above, and the default theme SHOULD style binding categories distinctly
from column categories.

Comments, strings, numbers, operators, keywords, and function names SHOULD keep
their existing semantic-token behavior except where parser-backed PDL name
classification intentionally identifies a parsed binding or column position.
The TextMate grammar remains a static fallback and is not required to reproduce
all parser-backed semantic-token categories.

### 17.5 Formatting

The formatter SHOULD use:

- one stage per line for multi-stage pipelines
- two spaces before leading `|`
- spaces around binary operators
- comma plus space between items
- one item per line for long item-list stages
- one assignment group per line for window-heavy `mutate` stages
- one line per `partition_by`, `order_by`, or `rows between ...` window clause
  when a window expression is expanded

Example formatted style:

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount)
  | save "out.csv"
```

Example expanded window style:

```pdl
load "sales.csv"
  | filter status == "completed"
  | mutate
      customer_sale_number =
        row_number() over (
          partition_by customer_id
          order_by amount desc
        ),
      customer_revenue =
        sum(amount) over (
          partition_by customer_id
        )
```

The current formatter withholds edits for documents containing comments because
comment attachment is not yet source-preserving. Short item lists SHOULD remain
inline so compact programs do not become unnecessarily vertical.

### 17.6 VS Code Client

The reference repository SHOULD include a VS Code extension under `editors/vscode/`.

Recommended VS Code client layout:

```text
editors/
  vscode/
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

The VS Code extension is a thin TypeScript language client.

It spawns `pdl lsp` by default.

It SHOULD expose settings:

- `pdl.server.path`
- `pdl.server.args`
- `pdl.trace.server`

`pdl.server.path` defaults to `pdl`.

`pdl.server.args` defaults to `["lsp"]`.

The VS Code extension MUST NOT implement PDL parsing, semantic analysis, diagnostics, completion, hover, formatting, or code actions.

Those features MUST come from the Rust LSP server.

The extension MAY include:

- TextMate grammar
- language configuration
- activation wiring
- packaging metadata

The TextMate grammar is only for static highlighting before semantic tokens arrive.

When static highlighting assets are shipped, the TextMate grammar SHOULD scope
implemented scalar, aggregate, and window function names under
`support.function.*.pdl`. Aggregate-style window calls such as
`sum(amount) over (...)` SHOULD receive a window function scope when the grammar
can identify the following `over` clause.

The language configuration is only for editor behaviors such as brackets, comments, indentation, and word patterns.

The extension SHOULD activate on `.pdl` files and the `pdl` language id.

The extension SHOULD register commands only for client wiring, such as restart server or show output channel.

The extension package version SHOULD align with workspace release version.

When PDL syntax changes, `editors/assets/pdl.tmLanguage.json` and
`editors/assets/language-configuration.json` MUST be updated as the canonical
static editor assets. The VS Code package MUST remain self-contained by syncing
those canonical assets into `editors/vscode/syntaxes/pdl.tmLanguage.json` and
`editors/vscode/language-configuration.json` before compile, lint, test,
package, and prepublish workflows.

## 18. Browser And WASM Runtime

### 18.1 WASM Crate

The reference workspace MUST include `pdl-wasm` if browser support is shipped.

The WASM runtime is an adapter over syntax, semantics, driver, data, exec, and editor-services crates.

It MUST NOT be a second parser.

It MUST NOT be a TypeScript semantic implementation.

### 18.2 Browser IO

Browser WASM operates on host-supplied in-memory files and streams.

It MUST NOT read arbitrary host files.

It MUST NOT fetch network resources by itself.

It MUST NOT inspect environment variables or process state.

It MUST NOT invoke external processes.

### 18.3 WASM JSON ABI

The WASM runtime SHOULD expose JSON ABI calls for:

- parse/check diagnostics
- parse/check diagnostics with host-supplied schemas
- formatting
- schema inspection
- plan inspection
- bounded in-memory execution
- Arrow IPC byte output
- editor-service requests for Monaco

Schema-aware WASM check calls MUST use host-supplied in-memory schemas or files
through the shared driver/editor-service path. They MUST NOT read arbitrary host
files or reimplement semantic validation in JavaScript or TypeScript.

The v0.24 WASM implementation MUST expose packed JSON calls for:

- `pdl_run_json`, which accepts PDL source, a host-supplied file map, a
  synthetic program path, and an optional requested stdout format, then prepares
  and executes through shared driver and exec code using in-memory driver IO
- `pdl_editor_service_json`, which accepts PDL source, the same host file map, a
  synthetic program path, and an editor-service request, then returns editor
  diagnostics plus the requested editor-service result

The browser run request's host file map is format-neutral and MAY contain
multiple files: keys are logical file paths and values are host-supplied file
contents. Version 0.26.0 requires CSV and JSON Lines host file contents to
execute successfully through this JSON ABI because host files are supplied as
UTF-8 strings. Binary host-file contents remain deferred until the ABI accepts
byte payloads. The ABI MUST NOT special-case CSV at the TypeScript editor layer.

`pdl_run_json` in version 0.26.0 MUST support CSV and JSON Lines stdout for the
resulting table when a stdout format is requested or when the document has no
named outputs and no stdout format is supplied. For documents with named output
declarations and no requested stdout format, `pdl_run_json` MUST return an
`outputs` array of `{ name, table }` entries in declaration order, where `table`
contains `columns` and string-rendered `rows`. The browser run facade MUST NOT
write path-backed `save` sinks to the native filesystem.

In version 0.26.0, `pdl_run_json` MUST additionally return a `files` object for
text path-backed `save` sinks inside named outputs. The object keys are logical
save paths as written by the PDL document, and the values are UTF-8 text
contents encoded with the sink's effective format. CSV and JSON Lines saves MUST
be eligible for this virtual file map. This field is additive: existing hosts
that read only `stdout`, `outputs`, `diagnostics`, and `error` MUST continue to
work. Binary virtual file payloads, Arrow IPC byte output, and binary dataframe
decoders remain deferred until a later plan promotes a byte-oriented browser
ABI.

Since version 0.29.0, `pdl_run_json` MAY accept a `context` object whose keys
are declared `param` or `state` names and whose values are JSON nulls, booleans,
numbers, or strings. The browser runtime MUST coerce these values to the same
typed context map used by native execution, fall back to declaration defaults
when a key is absent, and return registered diagnostics for unknown names,
non-primitive values, type mismatches, or invalid dynamic column resolution.

For hover requests, `pdl_editor_service_json` in version 0.26.0 MUST use the
same host file map through in-memory driver I/O so Monaco/WASM hover previews
match native LSP hover behavior for text-backed paths and columns.

Editor-service requests SHOULD use LSP-shaped positions and results.

The ABI boundary uses UTF-16 positions.

Internal spans remain byte offsets.

### 18.4 Monaco Host

The reference repository SHOULD include `demo/`.

The demo host SHOULD use Monaco.

The demo host MUST call the WASM editor-service ABI for language features.

The demo host MUST NOT implement a separate PDL parser or analyzer.

The demo host SHOULD reuse the VS Code TextMate grammar for static Monaco
highlighting and SHOULD style the full `support.function.*.pdl` scope family so
scalar, aggregate, ranking, and aggregate-style window functions are highlighted
consistently.

The v0.7 demo host MUST be a single-page React/Vite workbench with one PDL
Monaco editor, one host-supplied dataframe input display, one dataframe output
display, and diagnostics. It MAY use CSV text controls as the initial dataframe
display because CSV is the only implemented v0.7 data decoder.

The demo MAY show generated CSV, JSON Lines, Arrow, schema, manifest, and stream
handoff examples.

Docs-oriented live examples in the demo SHOULD keep the host-supplied input
fixture visible alongside the PDL source and resulting stdout when layout space
allows, so users can compare input rows with transformed output rows.

Since version 0.27.0, the reference repository MUST include package-shaped
browser integrations for npm-style consumption:

- `packages/wasm/` publishes the org-free package name `pdl-wasm` and owns
  runtime loading, ABI types, and helpers for caller-provided WASM URLs or a
  generated package-local `pdl.wasm` artifact.
- `editors/monaco/` publishes the org-free package name `pdl-editor` and owns
  Monaco language registration, TextMate grammar wiring, language
  configuration, theme defaults, marker conversion, editor-service provider
  registration, structural runtime/editor-service types, and a thin React
  editor component.

`pdl-wasm` MUST NOT include Monaco, React, demo UI, Studio UI, execution
buttons, output panels, or routing. `pdl-editor` MUST NOT implement PDL parsing,
analysis, diagnostics, completion, hover, formatting, semantic tokens, symbols,
definition/reference, or rename in TypeScript; it MUST adapt the upstream
WASM/editor-service ABI into Monaco providers.

Since version 0.28.0, those package surfaces MUST expose the expanded semantic
token categories defined by section 17.4. Browser hosts should receive binding
and column highlighting by updating `pdl-wasm`, `pdl-editor`, and the generated
WASM runtime rather than adding host-side PDL language logic.

The packages MUST support unpublished local development. In source mode, hosts
MAY install or alias sibling package directories from `../pdl` and pass an
explicit local `wasmUrl` for a generated artifact copied into the host's public
assets. In packed mode, release validation MAY build local tarballs into an
ignored package `dist/` or workspace `artifacts/` directory and install them
with `file:` paths before npm publication. Generated `pdl.wasm` binaries and
local package tarballs MUST NOT be checked into source.

Since version 0.30.0, published `pdl-wasm` and `pdl-editor` package manifests
MUST expose generated `dist/` entrypoints for `main`, `module`, `types`, and
`exports`, with CommonJS `dist/index.cjs`, ESM `dist/index.mjs`, and TypeScript
declarations. The `pdl-wasm` tarball MUST include `dist/pdl.wasm`; the
`pdl-editor` tarball MUST include generated `dist/` files plus package-local
static editor assets. `prepack` MUST build the publishable surface, and release
validation MUST inspect `npm pack --dry-run` output to prove ignored generated
`dist/` files are included by the npm `files` whitelist.

Since version 0.30.1, published `pdl-editor` `dist/` entrypoints MUST NOT emit
Vite-specific `?worker` or `?url` imports for Monaco workers or Onigasm WASM
assets. Browser hosts that want package-provided Monaco worker setup MUST pass
a `createEditorWorker` setup option, and hosts MUST pass an `onigasmWasmUrl`
setup option when using TextMate grammar loading.

Since version 0.39.0, the browser package version line aligns with the
Rust/CLI release when syntax, runtime, editor-service, or WASM-visible behavior
changes. `pdl-wasm@0.39.0` carries the updated parser and in-memory runtime, and
`pdl-editor@0.39.0` peers on `pdl-wasm@0.39.x` so Monaco hosts can consume the
matching editor-service ABI without a stale peer dependency range.

Version 0.40.0 is a native Rust/CLI release. It does not require browser npm
package manifests or consumer install pins to move past the latest verified
published `pdl-wasm@0.39.0` and `pdl-editor@0.39.0` packages unless those
packages are explicitly prepared and published.

Versions 0.42.0 and 0.43.0 are likewise native Rust/CLI releases. Browser
package publication stays independent: local package manifests carry the
workspace release version, while consumer dependency pins remain on the
latest verified published `pdl-wasm@0.39.0` and `pdl-editor@0.39.0`.

## 19. Rust Crate Architecture

### 19.1 Workspace Layout

PDL MUST follow a layered crate architecture that keeps syntax, semantics,
driver boundaries, execution, editor services, CLI, LSP, and WASM decoupled.

Recommended layout:

```text
pdl/
  Cargo.toml
  crates/
    pdl-cli/
    pdl-core/
    pdl-data/
    pdl-driver/
    pdl-editor-services/
    pdl-lsp/
    pdl-exec/
    pdl-semantics/
    pdl-syntax/
    pdl-wasm/
  docs/
  examples/
  tests/
  editors/
    vscode/
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
  demo/
```

For early implementation, a single crate is acceptable.

The design SHOULD keep module boundaries aligned with the future crates.

### 19.2 Cargo Manifest Templates

The standalone PDL repository SHOULD use one workspace manifest plus per-crate
manifests under `crates/`. All PDL crates MUST inherit package version,
edition, license, repository, and Rust version from `[workspace.package]`.

The root workspace manifest SHOULD start from this structure:

```toml
[workspace]
resolver = "2"
members = [
    "crates/pdl-core",
    "crates/pdl-syntax",
    "crates/pdl-data",
    "crates/pdl-semantics",
    "crates/pdl-driver",
    "crates/pdl-exec",
    "crates/pdl-editor-services",
    "crates/pdl-lsp",
    "crates/pdl-cli",
    "crates/pdl-bench",
    "crates/pdl-parity-tests",
    "crates/pdl-wasm",
]

[workspace.package]
version = "0.43.0"
edition = "2021"
license = "MIT OR Apache-2.0"
repository = "https://github.com/williamcotton/pdl"
rust-version = "1.80"

[workspace.dependencies]
# Internal crates
pdl-core = { path = "crates/pdl-core" }
pdl-syntax = { path = "crates/pdl-syntax" }
# Feature-bearing crates default native formats and the native dataframe engine
# off here so consumers, notably WASM, opt in explicitly. Native binaries
# re-enable them.
pdl-data = { path = "crates/pdl-data", default-features = false }
pdl-semantics = { path = "crates/pdl-semantics", default-features = false }
pdl-driver = { path = "crates/pdl-driver", default-features = false }
pdl-exec = { path = "crates/pdl-exec", default-features = false }
pdl-editor-services = { path = "crates/pdl-editor-services", default-features = false }
pdl-lsp = { path = "crates/pdl-lsp" }

# External dependencies
clap = { version = "4", features = ["derive"] }
logos = "0.14"
rowan = "0.16"
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
csv = "1"
chrono = "0.4"
indexmap = "2"
thiserror = "2"
polars = { version = "0.53", default-features = false, features = [
    "abs",
    "concat_str",
    "csv",
    "cum_agg",
    "fmt",
    "ipc_streaming",
    "lazy",
    "parquet",
    "rank",
    "round_series",
    "semi_anti_join",
    "strings",
    "temporal",
    "timezones",
] }
tower-lsp = "0.20"
lsp-types = "0.94.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "io-std", "sync"] }
dashmap = "6"
insta = "1"
pretty_assertions = "1"
parquet = "53"
arrow-array = "53"
arrow-schema = "53"
arrow-ipc = "53"
bytes = "1"
```

`crates/pdl-cli/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[[bin]]
name = "pdl"
path = "src/main.rs"

[dependencies]
pdl-core = { workspace = true }
pdl-syntax = { workspace = true }
pdl-semantics = { workspace = true }
pdl-driver = { workspace = true, features = ["native-formats"] }
pdl-data = { workspace = true, features = ["native-formats"] }
pdl-exec = { workspace = true, features = ["native-formats"] }
pdl-lsp = { workspace = true }
clap = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
arrow-array = { workspace = true }
arrow-schema = { workspace = true }
arrow-ipc = { workspace = true }
parquet = { workspace = true, features = ["arrow"] }
```

`crates/pdl-core/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-core"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
```

`crates/pdl-data/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-data"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
pdl-core = { workspace = true }
csv = { workspace = true }
chrono = { workspace = true }
indexmap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
polars = { workspace = true, optional = true }
parquet = { workspace = true, optional = true, features = ["arrow"] }
arrow-array = { workspace = true, optional = true }
arrow-schema = { workspace = true, optional = true }
arrow-ipc = { workspace = true, optional = true }
bytes = { workspace = true, optional = true }

[features]
default = ["native-formats"]
polars-engine = ["dep:polars"]
native-formats = ["polars-engine", "arrow-ipc", "parquet"]
arrow-ipc = ["dep:arrow-array", "dep:arrow-schema", "dep:arrow-ipc", "dep:bytes"]
parquet = ["dep:parquet", "arrow-ipc"]
```

`crates/pdl-driver/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-driver"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
pdl-core = { workspace = true }
pdl-syntax = { workspace = true }
pdl-data = { workspace = true }
pdl-semantics = { workspace = true }
thiserror = { workspace = true }

[features]
default = ["native-formats"]
native-formats = ["pdl-data/native-formats", "pdl-semantics/native-formats"]

[dev-dependencies]
tokio = { workspace = true }
```

`crates/pdl-editor-services/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-editor-services"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
pdl-core = { workspace = true }
pdl-syntax = { workspace = true }
pdl-semantics = { workspace = true }
pdl-driver = { workspace = true }
pdl-data = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }

[features]
default = ["native-formats"]
native-formats = ["pdl-data/native-formats", "pdl-semantics/native-formats", "pdl-driver/native-formats"]
```

`crates/pdl-lsp/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-lsp"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
pdl-core = { workspace = true }
pdl-syntax = { workspace = true }
pdl-semantics = { workspace = true }
pdl-driver = { workspace = true }
pdl-data = { workspace = true }
pdl-editor-services = { workspace = true, features = ["native-formats"] }
pdl-exec = { workspace = true, features = ["native-formats"] }
tower-lsp = { workspace = true }
tokio = { workspace = true }
dashmap = { workspace = true }
serde = { workspace = true }

[dev-dependencies]
futures-util = "0.3"
serde_json = { workspace = true }
tower-service = "0.3"
```

`crates/pdl-exec/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-exec"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
pdl-core = { workspace = true }
pdl-semantics = { workspace = true }
pdl-data = { workspace = true }
pdl-driver = { workspace = true }
chrono = { workspace = true }
indexmap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }

[features]
default = ["native-formats"]
native-formats = ["pdl-data/native-formats", "pdl-semantics/native-formats", "pdl-driver/native-formats"]
```

`crates/pdl-semantics/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-semantics"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
pdl-core = { workspace = true }
pdl-syntax = { workspace = true }
pdl-data = { workspace = true }

[features]
default = ["native-formats"]
native-formats = ["pdl-data/native-formats"]
```

`crates/pdl-syntax/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-syntax"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true

[dependencies]
pdl-core = { workspace = true }
logos = { workspace = true }
rowan = { workspace = true }
```

`crates/pdl-wasm/Cargo.toml` SHOULD start from:

```toml
[package]
name = "pdl-wasm"
version = { workspace = true }
edition = { workspace = true }
license = { workspace = true }
repository = { workspace = true }
rust-version = { workspace = true }
description = "Browser/WASM runtime for PDL: parse, analyze, plan, and run bounded in-memory pipelines."

[lib]
crate-type = ["cdylib", "rlib"]

# The WASM runtime excludes native format features by default. Browser hosts may
# opt into format support only when the selected dependencies build for wasm32.
[dependencies]
pdl-core = { workspace = true }
pdl-syntax = { workspace = true }
pdl-data = { workspace = true }
pdl-semantics = { workspace = true }
pdl-driver = { workspace = true }
pdl-exec = { workspace = true }
pdl-editor-services = { workspace = true }
lsp-types = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }

[[bin]]
name = "pdl-wasm-demo"
path = "src/bin/demo.rs"
```

Rendering, graphics, projection, and geometry-source dependencies MUST NOT be
added to PDL unless a later PDL feature requires them and the specification
documents that requirement.

### 19.3 Repository Automation

When hosted on GitHub, the reference repository SHOULD include a CI workflow
that checks Rust formatting, lints the full workspace, and runs the full Rust
workspace test suite.

When the browser demo is shipped, the reference repository SHOULD include a
GitHub Pages workflow that builds the WASM-backed Vite demo with a host-aware
base path and deploys the generated static site artifact.

The CI workflow SHOULD publish distributable release assets for shipped editor
and browser adapter outputs. When the VS Code client and WASM runtime are
shipped, CI SHOULD package the VS Code extension `.vsix` and the standalone
`pdl.wasm` runtime, verify that their package versions match the Rust workspace
release version, and upload the files to the GitHub Release tagged for that
version. The release SHOULD contain both versioned filenames and `latest`
aliases.

Repository automation MUST keep stdout data stream requirements intact. CI logs
and deployment logs are host concerns and do not alter PDL runtime stdout
semantics.

### 19.4 Module Boundaries

`pdl-core` owns:

- spans
- diagnostics
- severity
- source IDs
- shared result types
- stable diagnostic code definitions

`pdl-syntax` owns:

- lexer
- parser
- CST
- AST
- parse diagnostics
- formatter
- byte-span helpers

`pdl-data` owns:

- Polars-backed dataframe abstraction
- scalar values
- logical schema types
- CSV loading and writing
- Arrow IPC reading and writing
- Parquet support where enabled
- JSON Lines support where enabled
- format-specific row diagnostics

`pdl-driver` owns:

- source-expression extraction
- source-relative path resolution
- stdin/stdout source descriptors
- format inference and sniffing
- load-free pipeline data plan
- schema loading orchestration
- injectable IO provider
- schema cache
- dependency inventory
- centralized data error to diagnostic mapping
- preparation reports shared by CLI, LSP, WASM, and tests

`pdl-semantics` owns:

- name resolution
- binding graph validation
- schema-aware validation
- expression type checking
- stage registry
- function registry
- semantic IR
- semantic diagnostics

`pdl-exec` owns:

- target selection
- execution planning
- streaming execution
- blocking stage materialization
- Polars-backed transform execution through `pdl-data`
- Polars-backed aggregate execution through `pdl-data`
- Polars-backed join execution through `pdl-data`
- artifact writes
- deterministic materialization of tables to output formats
- run manifest construction
- cache-key construction

`pdl-exec` also owns deterministic external output emission:

- CSV rendering
- Arrow IPC stream rendering
- Arrow IPC file rendering
- Parquet output where enabled
- JSON Lines output where enabled
- run manifest rendering
- plan and schema JSON rendering
- human preview table rendering

`pdl-exec` executes the analyzed pipeline and turns internal table results into
deterministic external artifacts.

PDL SHOULD use `pdl-exec` for this crate because the language executes data pipelines rather than rendering graphics.

`pdl-editor-services` owns:

- completion
- hover
- signature help
- semantic tokens
- code actions
- navigation
- references
- rename
- document symbols
- editor-neutral diagnostics helpers
- schema-aware diagnostics over driver preparation reports
- conversion between byte spans and editor-neutral UTF-16 text ranges

`pdl-lsp` owns:

- tower-lsp backend
- document cache
- LSP transport
- request routing
- cancellation
- conversion from editor-service ranges and diagnostics into LSP protocol types

`pdl-cli` owns:

- argument parsing
- command dispatch
- OS-backed IO construction
- process exit codes
- human and JSON terminal output

`pdl-wasm` owns:

- browser-embeddable runtime entry points
- in-memory driver IO integration
- editor-service JSON ABI
- check/schema/plan/format facades
- bounded in-memory run facade

### 19.4 Dependency Guidelines

PDL SHOULD prefer established Rust ecosystem crates for Cargo workspace
conventions, CLI parsing, resilient syntax trees, serde-based JSON,
CSV/Arrow/Parquet data handling, LSP transport, async runtime, stable ordering,
diagnostics, and snapshot/test helpers unless the PDL specification documents a
deliberate substitution.

Recommended dependencies:

- `clap` for CLI
- `logos` for lexing
- `rowan` for lossless CST
- `serde` and `serde_json` for debug JSON and manifests
- `csv` for CSV
- `arrow` or compatible Arrow crates for Arrow IPC
- `parquet` for Parquet
- `polars` for the internal dataframe engine and transformation execution
- `chrono` or `time` for temporal parsing
- `indexmap` for stable ordering
- `thiserror` for internal errors
- `tower-lsp` for LSP
- `tokio` for async LSP runtime
- `dashmap` for concurrent LSP caches
- `insta` for snapshots
- `similar` or `pretty_assertions` for test diffs

Polars MUST be used behind `pdl-data` for dataframe transformations in the
reference implementation.

Polars MUST NOT leak into parser, syntax, semantic analysis, editor-services, LSP protocol, or source language semantics.

The driver crate MUST NOT depend on CLI or LSP crates.

The editor-services crate MUST NOT depend on LSP transport.

The WASM crate MUST not require native-only features unless gated.

### 19.5 Error Handling

Internal errors use Rust `Result`.

User diagnostics are values.

Parser returns syntax tree plus diagnostics.

Analyzer returns optional IR plus diagnostics.

Driver returns preparation reports plus diagnostics.

Exec returns artifact output plus diagnostics or structured execution error.

Panic is reserved for programmer bugs.

CLI catches top-level errors and prints concise messages.

LSP logs internal errors and avoids crashing where possible.

### 19.6 Implementation Patterns

The standalone PDL repository SHOULD preserve implementation patterns that
support deterministic analysis, resilient editor behavior, and shared
CLI/LSP/WASM contracts.

The exact Rust type names are guidance, but the roles and crate boundaries in
this section MUST exist in the reference implementation once the corresponding
feature is implemented.

#### 19.6.1 Lossless Syntax Model

`pdl-syntax` MUST use a lossless CST as the parser's primary output.

The CST MUST retain trivia, token kinds, and byte spans.

Typed AST wrappers SHOULD be cheap views over CST nodes and tokens.

Typed AST wrappers MUST NOT own semantic facts, inferred schemas, Polars values,
or resolved binding IDs.

The syntax crate SHOULD expose a parse result shaped like:

```rust
pub struct Parse {
    pub green: rowan::GreenNode,
    pub diagnostics: Vec<Diagnostic>,
}
```

AST wrapper APIs SHOULD expose optional children rather than panicking on missing
syntax.

Formatter, LSP semantic tokens, hover range selection, and code actions SHOULD
derive source ranges from the same CST and AST wrappers used by parsing tests.

#### 19.6.2 Resilient Parse Recovery

The parser MUST recover and continue after malformed input where practical.

PDL parser recovery SHOULD synchronize at:

- pipe tokens
- top-level `let`
- stage keywords
- commas in argument lists
- assignment targets
- join `on` clauses
- newline boundaries where a new pipeline can begin
- EOF

A missing stage MUST produce `E0006` and a partial syntax node when a partial
node lets editor features continue.

A malformed stage MUST produce the most specific applicable syntax or stage
diagnostic, such as `E1201`, `E1203`, or `E1206`.

The parser MUST NOT perform schema lookup, file IO, Polars planning, or semantic
validation.

#### 19.6.3 Diagnostic Data Model

Diagnostics are data, not exceptions.

All diagnostics emitted by syntax, data, driver, semantics, exec, CLI, LSP, or
WASM MUST use the same core diagnostic structure.

The diagnostic structure SHOULD be shaped like:

```rust
pub struct DiagnosticCode(&'static str);

pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub related: Vec<RelatedSpan>,
    pub help: Option<String>,
}

pub struct RelatedSpan {
    pub span: Span,
    pub message: String,
}
```

Registered code constants SHOULD live under `pdl_core::codes`.

Example:

```rust
use pdl_core::{codes, Diagnostic, Span};

let diagnostic = Diagnostic::error(
    codes::E1005,
    "unknown column",
    Span::new(42, 47),
);
```

`Span` MUST be a half-open byte range into a known source document.

Diagnostics that point into generated schemas, manifests, or host-provided
metadata MAY use an implementation-defined sentinel span such as `Span::zero()`,
but they MUST still carry a stable diagnostic code.

CLI human output, CLI JSON output, LSP diagnostics, WASM JSON payloads, and
snapshot tests MUST derive from this same diagnostic data.

#### 19.6.4 Semantic Analysis Pipeline

Semantic analysis SHOULD run in explicit phases.

Phase one resolves top-level bindings and builds a deterministic binding graph.

Phase two validates each referenced pipeline in dependency order.

Phase three lowers validated syntax into semantic IR.

Phase four produces a stage-by-stage schema trace for diagnostics, editor
features, planning, and manifests.

The analyzer SHOULD keep these structures distinct:

```rust
pub struct BindingGraph {
    pub bindings: IndexMap<BindingId, BindingInfo>,
    pub edges: Vec<BindingEdge>,
}

pub struct StageTrace {
    pub stage_id: StageId,
    pub input_schema: Option<TableSchema>,
    pub output_schema: Option<TableSchema>,
    pub grouping: GroupingState,
    pub ordering: OrderingState,
    pub diagnostics: Vec<Diagnostic>,
}
```

Binding and stage IDs MUST be deterministic for identical source input.

Cycle detection, unknown binding diagnostics, and duplicate binding diagnostics
MUST be reported before execution planning.

#### 19.6.5 Stage Schema Transitions

Every stage MUST define a schema transition contract.

A stage transition consumes the current analysis state and returns the next
analysis state plus diagnostics.

The contract SHOULD be shaped like:

```rust
pub struct StageInput<'a> {
    pub schema: Option<&'a TableSchema>,
    pub grouping: GroupingState,
    pub ordering: OrderingState,
}

pub struct StageOutput {
    pub schema: Option<TableSchema>,
    pub grouping: GroupingState,
    pub ordering: OrderingState,
    pub diagnostics: Vec<Diagnostic>,
}
```

`filter` MUST preserve input schema.

`select`, `drop`, and `rename` MUST deterministically transform column order.

`group_by` MUST update grouping state without aggregating rows.

`agg` MUST consume grouping state and produce a new schema ordered by group keys
followed by aggregate outputs.

`join` MUST validate key presence and deterministic column collision behavior
before planning execution. Implementations with static key type facts MUST also
validate key type compatibility before planning; the CSV-backed reference
implementation validates observed key value classes during execution.

`save` MUST preserve the input schema for analysis while adding a sink artifact
to the plan.

Schema transition code MUST live in `pdl-semantics` or a registry owned by
`pdl-semantics`; it MUST NOT depend on Polars.

#### 19.6.6 Registries

PDL SHOULD use central registries for language facts that affect multiple
crates.

The stage registry SHOULD define:

- accepted stage names
- positional and named argument shapes
- whether a stage can start a pipeline
- whether a stage is terminal
- schema transition behavior
- completion text
- hover documentation
- diagnostic codes used by the stage

The function registry SHOULD define:

- accepted scalar and aggregate functions
- argument arity
- input type constraints
- output type rules
- determinism
- aggregate-only or row-expression-only restrictions

The format registry SHOULD define:

- stable format names
- path extensions
- sniffing predicates
- load support
- save support
- stream support
- CLI aliases
- diagnostic codes used by format detection

The syntax lexer MAY keep token-level keyword tables locally, but semantic,
editor-service, CLI help, and documentation generation SHOULD consume shared
registry metadata where practical.

#### 19.6.7 Semantic IR And Polars Boundaries

Semantic IR MUST be independent of parser syntax nodes and independent of
Polars.

IR nodes SHOULD carry stable IDs, source spans, resolved binding IDs, resolved
column references, stage options, and inferred output schemas.

The IR SHOULD represent PDL concepts directly:

```rust
pub enum StageIr {
    Load(LoadIr),
    Filter(FilterIr),
    Select(SelectIr),
    Drop(DropIr),
    Rename(RenameIr),
    GroupBy(GroupByIr),
    Agg(AggIr),
    Sort(SortIr),
    Limit(LimitIr),
    Join(JoinIr),
    Union(UnionIr),
    Distinct(DistinctIr),
    Save(SaveIr),
}
```

`pdl-exec` SHOULD lower semantic IR into an execution plan.

`pdl-data` MUST own the concrete Polars `DataFrame`, `LazyFrame`, expression,
and IO adapter usage.

`pdl-exec` MAY call `pdl-data` facade methods that internally build Polars lazy
plans, but it SHOULD NOT expose Polars types in public APIs consumed by CLI,
LSP, WASM, or editor-services.

#### 19.6.8 Driver Preparation Report

The driver SHOULD provide one preparation path shared by CLI, LSP, WASM, tests,
and embedded callers.

The preparation report SHOULD collect entries in deterministic phase order:

- parse
- source resolution
- schema facts and bounded metadata loading
- semantic analysis
- planning
- execution
- output

The report SHOULD be shaped like:

```rust
pub struct PreparationReport {
    pub parse: ParseSummary,
    pub sources: Vec<SourceDependency>,
    pub schemas: Vec<SchemaReport>,
    pub semantics: Option<SemanticSummary>,
    pub plan: Option<PlanSummary>,
    pub diagnostics: Vec<Diagnostic>,
}
```

The driver MUST continue through recoverable phase boundaries when later phases
can still produce useful diagnostics or editor facts.

The driver MUST NOT short-circuit after the first diagnostic unless the input is
unusable for all later phases.

#### 19.6.9 Injectable IO

Filesystem, stdin, stdout, and host-provided bytes MUST be isolated behind
driver or exec IO abstractions.

The driver IO abstraction SHOULD support:

- reading path-backed source bytes
- reading path-backed data bytes
- reading stdin bytes
- checking source fingerprints
- resolving source-relative paths
- listing dependencies needed by LSP and manifests

Native CLI code MAY provide an OS-backed implementation.

Tests, WASM, browser demos, and embedded callers SHOULD provide in-memory
implementations.

Parser, semantic analysis, editor-services, and LSP transport MUST NOT directly
read files or process stdin.

#### 19.6.10 Deterministic Collections And Snapshots

Implementation code SHOULD use insertion-ordered or explicitly sorted
collections for user-visible output.

This applies to diagnostics, binding lists, column lists, function completions,
stage completions, format lists, schema JSON, manifest JSON, plan JSON, and
snapshot output.

Tests SHOULD snapshot:

- token streams for representative syntax
- CST debug trees for valid and invalid syntax
- parser diagnostics and byte spans
- semantic diagnostics and stage traces
- IR JSON for representative pipelines
- preparation reports
- plan JSON
- manifest JSON
- CSV and Arrow output round trips

Parser, analyzer, driver, and LSP tests MUST include non-ASCII source text when
asserting spans or UTF-16 LSP ranges.

### 19.7 v0.5 Architecture Audit

This section records the v0.5 architecture audit and boundary decisions.

#### 19.7.1 Dependency Direction

Allowed internal dependency direction:

```text
pdl-core
  <- pdl-syntax
  <- pdl-data
  <- pdl-semantics
  <- pdl-driver
  <- pdl-exec
  <- pdl-editor-services
  <- pdl-lsp
  <- pdl-cli
  <- pdl-wasm
```

Practical rules:

- `pdl-core` owns shared primitives and MUST NOT depend on any other PDL
  internal crate.
- `pdl-syntax` depends only on syntax concerns plus `pdl-core`.
- `pdl-semantics` may depend on `pdl-syntax`, `pdl-data`, and `pdl-core`, but
  only through stable logical schema/type facts, never concrete engine types.
- `pdl-driver` owns source, path, stream, format, schema facts, and preparation
  boundaries.
- `pdl-exec` owns executable planning, table execution, output emission, and
  manifest/preview surfaces.
- `pdl-editor-services`, `pdl-lsp`, `pdl-cli`, and `pdl-wasm` are adapters.

The reference implementation includes boundary tests that read workspace
manifests and selected source files so dependency-direction drift and concrete
engine leakage fail during `cargo test --workspace`.

#### 19.7.2 Public API Audit

No public API above `pdl-data` may mention Polars `DataFrame`, `LazyFrame`,
Polars expressions, Arrow reader internals, Parquet reader internals, or native
format engine details.

Stable data-facing surfaces are:

- `pdl_data::Table`
- `pdl_data::Row`
- `pdl_data::Value`
- `pdl_data::TableSchema`
- `pdl_data::ColumnSchema`
- `pdl_data::LogicalType`
- `pdl_data::DataFormat`

`pdl-exec` may call `pdl-data` facade methods and plain table operations. It
must not construct concrete Polars or Arrow engine objects directly.

#### 19.7.3 Driver Plan

`pdl-driver` builds a load-free `DriverPlan` during preparation. It records:

- source origin and source path;
- base directory;
- pipeline input descriptors;
- output sink descriptors;
- explicit format names;
- inferred path formats;
- deferred sniffing decisions;
- stdin/stdout stream uses;
- source spans;
- dependency inventory.

Plan construction does not read full data files and does not consume stdin. The
`DriverIo` trait is limited to local source bytes, data bytes, stdin bytes,
path metadata, and in-memory host-provided files. It intentionally has no
network, environment, shell, async, or cache policy methods.

#### 19.7.4 Semantic IR Handoff

`pdl-semantics` lowers parsed programs into `ProgramIr`, `PipelineIr`, and
`StageIr`. `pdl-exec` builds execution plans and executes the implemented CSV
slice from semantic IR plus `DriverPlan` facts rather than inspecting source
AST stages.

#### 19.7.5 Phase Reports

Preparation reports use the fixed phase order:

```text
parse -> source-resolution -> schema-facts -> semantic -> planning -> execution -> output
```

Schema-loading diagnostics are attributed to `schema-facts`. Semantic
diagnostics remain core diagnostic values and are not duplicated into the
report when schema facts already own the failure.

#### 19.7.6 Implementation Pattern Audit

PDL keeps these implementation lessons:

- lossless syntax and typed AST views;
- diagnostics as values;
- phase-tagged preparation reports;
- local driver I/O seam;
- editor/LSP thinness;
- browser-safe WASM host boundaries;
- deterministic outputs;
- planning before emission.

PDL intentionally diverges here:

- tabular execution replaces graphics rendering;
- `pdl-data` owns dataframe and native format privacy;
- source/sink descriptors prepare for Arrow stdout discipline;
- render-level asset loading and graphics-specific dependencies are not copied.

#### 19.7.7 Arrow Source And Sink Sketch

The driver/exec boundary can describe these formats without claiming runtime
support:

- `csv`
- `arrow-stream`
- `arrow-file`
- `parquet`
- `jsonl`

Descriptors record explicit format names, inferred path formats, and unresolved
sniffing decisions. Native Arrow IPC stream parsing/writing and stdin sniffing
are implemented in v0.15.0. Native Parquet loading/saving, Arrow IPC file
parity, and JSON Lines loading/saving are implemented in v0.15.0. Browser Arrow
byte output remains deferred unless a future plan promotes it with spec,
examples, and tests.

#### 19.7.8 Schema Cache And Preview Boundary

Schema cache keys must use resolved source identity plus a fingerprint or
host-provided version. Path strings alone are not valid cache keys.

The v0.5 code exposes:

- `SchemaCacheKey`
- `SourceIdentity`
- `SchemaCacheEntry`
- `PreviewRequest`

The cache boundary stores schemas and load errors, not full frames. Runtime
frame caching remains deferred. LSP/editor flows may opt out of runtime cache
use or rely on host-provided schemas when source data is not available locally.

#### 19.7.9 Adapter Thinness

CLI owns argument parsing, exit codes, stdout/stderr policy, and OS-backed file
writes. LSP owns protocol conversion and document lifecycle. Editor services
own protocol-neutral language features. VS Code only spawns/configures `pdl
lsp`. WASM uses in-memory driver/exec boundaries and does not read arbitrary
host files, process stdin, environment variables, network resources, or
external processes.

## 20. Diagnostics Catalog

### 20.1 Code Families

Diagnostic codes are grouped:

- `E0001`-`E0099`: lexical and syntax errors
- `E1001`-`E1099`: binding and scope errors
- `E1101`-`E1199`: column and schema errors
- `E1201`-`E1299`: stage and format errors
- `E1301`-`E1399`: type errors
- `E1401`-`E1499`: expression, function, aggregate, and window errors
- `E1501`-`E1599`: graph and planning errors
- `E1601`-`E1699`: CLI and invocation errors
- `E1701`-`E1799`: materialization and output errors
- `E1801`-`E1899`: data source and format runtime errors
- `E1901`-`E1999`: stream interop errors
- `E2001`-`E2099`: reactive context declaration and runtime errors
- `W2001`-`W2099`: author-facing warnings
- `H3001`-`H3099`: author-facing hints
- `R4001`-`R4099`: implementation-oriented runtime/internal diagnostics

The implementation MUST NOT emit an unregistered code.

When a condition could fit multiple codes, the most specific source-facing code
wins. For example, an unknown column in `select` is `E1005`; it is not a generic
stage-argument error.

### 20.2 Syntax Diagnostics

`E0001` unexpected token.

`E0002` unterminated string token or escaped column reference.

`E0003` unterminated block comment.

`E0004` invalid escape sequence.

`E0005` invalid character.

`E0006` missing stage after pipe.

`E0007` expected pipeline start.

`E0008` expected expression.

`E0009` expected column name.

`E0010` expected binding name.

`E0011` expected string token.

`E0012` expected function call.

`E0013` expected comma or closing delimiter.

`E0014` expected assignment target.

`E0015` expected `=`.

`E0016` expected format name.

`E0017` expected integer literal.

`E0018` malformed sort item.

`E0019` malformed window clause.

`E0020` unmatched delimiter.

`E0021` trailing tokens after pipeline.

`E0022` malformed `let` binding.

`E0023` expected stage name.

`E0024` expected source or sink target.

`E0025` required language feature is not enabled.

`E0026` double-quoted token is not a column reference in v0.26 syntax.

`E0027` legacy `as` alias syntax is not valid in v0.26 syntax.

### 20.3 Binding, Scope, Column, And Schema Diagnostics

`E1001` duplicate top-level name.

`E1002` reserved keyword used as binding.

`E1003` binding name conflicts with generated artifact name.

`E1004` binding is not a pipeline value.

`E1005` unknown column.

`E1006` ambiguous column.

`E1007` unknown binding.

`E1008` duplicate source column.

`E1009` schema unavailable for column resolution.

`E1010` duplicate output column name.

`E1011` column reference is not valid in this context.

`E1012` provisional schema required.

`E1013` duplicate schema field.

`E1014` schema field type is unsupported.

`E1015` schema field nullability is incompatible.

### 20.4 Stage And Format Diagnostics

`E1201` unknown stage.

`E1202` stage used in invalid position.

`E1203` missing required stage argument.

`E1204` unknown stage option.

`E1205` duplicate stage option or duplicate stage item.

`E1206` invalid stage argument type.

`E1207` output column collision.

`E1208` incompatible join keys.

`E1209` incompatible union schemas.

`E1210` invalid enum value.

`E1211` unsupported optional construct.

`E1212` invalid grouping state.

`E1213` invalid aggregate context.

`E1214` invalid sort specification.

`E1215` unknown format.

`E1216` format inference failed.

`E1217` explicit format conflicts with CLI override.

`E1218` duplicate save option.

`E1219` invalid save option.

`E1220` missing load source.

`E1221` missing save sink.

`E1222` negative limit.

`E1223` invalid join kind.

`E1224` invalid CSV dialect option.

`E1225` unsupported format alias.

`E1226` window expression is not valid in this context.

### 20.5 Type Diagnostics

`E1301` unknown type.

`E1302` incompatible operand types.

`E1303` invalid assignment type.

`E1304` invalid cast.

`E1305` unsupported implicit coercion.

`E1306` numeric overflow.

`E1307` invalid temporal value.

`E1308` nullability violation.

`E1309` ambiguous type inference.

`E1310` predicate is not boolean.

`E1311` sort key type is not orderable.

`E1312` grouping key type is not hashable or comparable.

`E1313` incompatible output schema.

### 20.6 Expression Diagnostics

`E1401` unknown function.

`E1402` wrong function arity.

`E1403` invalid function argument type.

`E1404` function not allowed in context.

`E1405` non-deterministic function not allowed.

`E1406` invalid literal value.

`E1407` divide by zero detected statically.

`E1408` comparison chain not supported.

`E1409` invalid window specification.

`E1410` aggregate function not allowed in row context.

`E1411` scalar function not allowed in aggregate context.

`E1412` invalid aggregate argument.

`E1413` invalid window frame bound.

`E1414` ranking or offset window function requires `order_by`.

`E1415` explicit frame is not allowed for this window function.

`E1416` window function is not allowed in aggregate context.

`E1417` aggregate item requires assignment.

### 20.7 Planning Diagnostics

`E1501` binding dependency cycle.

`E1502` no runnable main pipeline.

`E1503` ambiguous or unavailable runnable target.

`E1504` cache entry invalid.

`E1505` unsupported plan node.

`E1506` unsupported streaming plan.

`E1507` blocking stage cannot run in requested mode.

`E1508` selected target is not runnable.

`E1509` manifest dependency resolution failed.

`E1510` planning limit exceeded.

### 20.8 CLI Diagnostics

`E1601` unknown CLI command.

`E1602` unknown CLI option.

`E1603` missing CLI argument.

`E1604` invalid CLI argument value.

`E1605` conflicting CLI options.

`E1606` no input pipeline was provided.

`E1607` stdout data stream would be mixed with human output.

`E1608` requested subcommand is not implemented.

`E1609` selected binding was not found.

`E1610` current working directory is unavailable.

`E1611` requested runtime feature is unavailable.

### 20.9 Materialization And Output Diagnostics

`E1701` invalid output path.

`E1702` output exists and overwrite is false.

`E1703` output directory unavailable.

`E1704` write failed.

`E1705` unsupported output format.

`E1706` stdout unavailable.

`E1707` overwrite is not supported for this sink.

`E1708` manifest write failed.

`E1709` output encoding failed.

`E1710` output row or byte limit exceeded.

### 20.10 Data Source Runtime Diagnostics

`E1801` source file not found.

`E1802` source file unreadable.

`E1803` source encoding invalid.

`E1804` row parse failed.

`E1805` source schema mismatch.

`E1806` stdin unavailable.

`E1807` stream sniffing failed.

`E1808` unsupported Parquet logical type.

`E1809` CSV header missing.

`E1810` duplicate source column.

`E1811` row width does not match header width.

`E1812` source format parse failed.

`E1813` JSON Lines row is not an object.

`E1814` unsupported Arrow schema.

`E1815` unsupported Parquet column type.

`E1816` source path is outside allowed roots.

`E1817` source fingerprint changed during preparation.

`E1818` source schema unavailable.

### 20.11 Stream Interop Diagnostics

`E1901` stream handoff format unsupported.

`E1902` stream interop manifest failed.

`E1903` stream handoff format conflict.

`E1904` stream handoff path invalid.

`E1905` consumer format could not be inferred.

### 20.12 Reactive Context Diagnostics

`E2001` duplicate context declaration.

`E2002` unknown or mismatched parameter/state reference.

`E2003` invalid context declaration default.

`E2004` invalid context value in a column-indirection position.

`E2005` external context value type mismatch.

### 20.13 Warning Diagnostics

`W2001` active grouping state was not consumed by `agg`.

`W2002` ambiguous quoted token. Retired from active v0.26 syntax; reserved for
pre-v0.26 compatibility diagnostics.

`W2003` dropping all columns.

`W2004` `limit` follows a stage with unstable order.

`W2005` format sniffing used a fallback.

`W2006` unsupported optional construct was ignored.

`W2007` rows were dropped due to missing values.

`W2008` default null ordering was applied.

`W2009` output overwrite was requested.

`W2010` schema is provisional.

`W2011` stage may require full materialization.

`W2012` source format was inferred from extension only.

### 20.14 Hint Diagnostics

`H3001` use `col(...)` or `lit(...)` to disambiguate a quoted token. Retired
from active v0.26 syntax; reserved for pre-v0.26 compatibility diagnostics.

`H3011` write a simple column reference as a bare identifier.

`H3012` write a non-simple or reserved column reference with backticks.

`H3002` add `agg` after `group_by`.

`H3003` add `sort` before `limit` for deterministic top-N output.

`H3004` add an explicit `format` clause.

`H3005` add a tie-breaker column to window `order_by`.

`H3006` use `arrow-stream` for stream handoff.

`H3007` use `--stdout-format` when piping data.

`H3008` add an explicit assignment target to avoid an output column collision.

`H3009` use the canonical lowercase format name.

`H3010` add an explicit overwrite option when replacing files.

`H3011` use a bare identifier or backticks for a column reference.

`H3012` write aliases as `new_name = expression`.

### 20.15 Internal Runtime Diagnostics

`R4001` internal invariant violation.

`R4002` execution engine error not attributable to source.

`R4003` dataframe backend error was not classified.

`R4004` serialization adapter failure.

`R4005` LSP document cache inconsistency.

`R4006` WASM host ABI contract violation.

`R4007` diagnostic code is not registered.

`R4008` preview execution budget exceeded after planning.

Internal diagnostics SHOULD NOT be used when an author-facing `E`, `W`, or `H`
code can describe the condition.

## 21. Testing Strategy

### 21.1 Test Categories

A PDL implementation SHOULD include:

- lexer tests
- parser tests
- formatter tests
- analyzer tests
- type checker tests
- stage schema tests
- stage execution tests
- source format tests
- format sniffing tests
- CLI tests
- LSP tests
- WASM ABI tests
- stream interop tests
- security tests
- performance tests

### 21.2 Parser Tests

Parser tests MUST cover valid syntax and malformed syntax.

Tests MUST assert source spans.

Tests MUST include non-ASCII text to verify byte offsets.

### 21.3 Stage Tests

Each stage SHOULD have:

- schema tests
- execution tests
- null behavior tests
- ordering tests
- diagnostic tests

### 21.4 Format Tests

Format tests MUST cover CSV and Arrow IPC stream.

Reference implementation tests SHOULD cover Parquet.

Sniffing tests MUST verify that consumed peek bytes are not lost.

### 21.5 CLI Tests

CLI tests SHOULD verify stdout contains only data in data-output mode.

Diagnostics and logs SHOULD be asserted on stderr.

Pipeline-to-consumer interop tests MAY use a fake consumer that validates Arrow IPC bytes.

### 21.6 LSP Tests

LSP tests SHOULD cover diagnostics, completion, hover, semantic tokens, formatting, go to definition, references, rename, and document symbols.

Tests MUST cover UTF-16 position conversion with non-ASCII source.

### 21.7 WASM Tests

WASM tests SHOULD cover browser-safe check, format, schema, plan, and editor-service requests.

WASM tests MUST verify no native filesystem or process execution dependency is required.

## 22. Performance

PDL SHOULD stream where possible.

CSV reading SHOULD be buffered.

Arrow IPC streaming SHOULD avoid unnecessary copies.

Parquet readers SHOULD use projection pushdown where practical.

Filter pushdown MAY be implemented when semantics are preserved.

Blocking stages SHOULD be visible in plans.

The LSP SHOULD avoid full data reads on the hot path.

The LSP SHOULD cap schema preview reads.

The runtime SHOULD expose limits for maximum rows, maximum bytes, maximum diagnostics, and maximum memory where practical.

## 23. Security

PDL source MUST NOT execute arbitrary code.

PDL source MUST NOT execute shell commands.

PDL source MUST NOT fetch network resources by default.

External process execution is outside PDL v0.1 source semantics.

Paths MUST be normalized before access.

Sandboxed runtimes MUST enforce allowed roots.

WASM runtimes MUST use host-provided in-memory files.

Secret management is deferred.

Diagnostics MUST avoid dumping large row values by default.

Regex functions, if added, MUST avoid catastrophic backtracking.

## 24. Versioning

PDL source does not require an explicit version declaration in draft 0.43.0.

The implementation SHOULD report supported language version.

Patch releases SHOULD preserve syntax and semantics.

Minor releases MAY add stages, functions, formats, and diagnostics.

Breaking changes require a major version after 1.0.

Diagnostic codes MUST remain stable after release.

Manifest schemas MUST include a manifest version.

## 25. Appendix A: Complete EBNF Draft

```ebnf
Program          ::= Trivia* BindingDecl* OutputDecl* PipelineExpr? Trivia* EOF ;
BindingDecl      ::= "let" Ident "=" PipelineExpr ;
OutputDecl       ::= "output" Ident "=" PipelineExpr ;
PipelineExpr     ::= PipelineStart PipelineTail* ;
PipelineStart    ::= LoadStage | Ident ;
PipelineTail     ::= "|" Stage ;
Stage            ::= FilterStage
                   | SelectStage
                   | DropStage
                   | RenameStage
                   | MutateStage
                   | GroupByStage
                   | AggStage
                   | SortStage
                   | LimitStage
                   | JoinStage
                   | UnionStage
                   | DistinctStage
                   | PivotLongerStage
                   | CompleteStage
                   | SaveStage ;

LoadStage        ::= "load" SourceRef FormatClause? ;
SourceRef        ::= StringToken | "stdin" | "-" ;
SaveStage        ::= "save" SinkRef FormatClause? SaveOptions? ;
SinkRef          ::= StringToken | "stdout" | "-" ;
FormatClause     ::= "format" FormatName ;
FormatName       ::= StringToken | Ident ;

FilterStage      ::= "filter" PredicateExpr ;
SelectStage      ::= "select" SelectItem ("," SelectItem)* ;
SelectItem       ::= ColumnRef | ColumnName "=" ColumnRef ;
DropStage        ::= "drop" ColumnRef ("," ColumnRef)* ;
RenameStage      ::= "rename" RenameItem ("," RenameItem)* ;
RenameItem       ::= ColumnName "=" ColumnRef ;
MutateStage      ::= "mutate" Assignment ("," Assignment)* ;
Assignment       ::= ColumnName "=" ValueExpr ;
GroupByStage     ::= "group_by" ColumnRef ("," ColumnRef)* ;
AggStage         ::= "agg" AggItem ("," AggItem)* ;
AggItem          ::= ColumnName "=" AggCall ;
AggCall          ::= Ident "(" ArgList? ")" ;
SortStage        ::= "sort" SortItem ("," SortItem)* ;
SortItem         ::= ColumnRef SortDirection? NullsOrder? ;
SortDirection    ::= "asc" | "desc" ;
NullsOrder       ::= "nulls_first" | "nulls_last" ;
LimitStage       ::= "limit" IntLiteral ;
JoinStage        ::= "join" JoinSource JoinOn JoinKind? ;
JoinSource       ::= Ident ;
JoinOn           ::= "on" JoinKey ("," JoinKey)* ;
JoinKey          ::= ColumnRef | "(" ColumnRef "," ColumnRef ")" ;
JoinKind         ::= "kind" JoinKindName ;
JoinKindName     ::= "inner" | "left" | "right" | "full" | "semi" | "anti" ;
UnionStage       ::= "union" Ident UnionOptions? ;
UnionOptions     ::= UnionOption* ;
UnionOption      ::= "by_name" BoolLiteral | "distinct" BoolLiteral ;
DistinctStage    ::= "distinct" ColumnRefList? ;
ColumnRefList    ::= ColumnRef ("," ColumnRef)* ;
PivotLongerStage ::= "pivot_longer" ColumnRefList
                     "names_to" ColumnName "values_to" ColumnName ;
CompleteStage    ::= "complete" ColumnRefList CompleteFill? ;
CompleteFill     ::= "fill" CompleteFillItem ("," CompleteFillItem)* ;
CompleteFillItem ::= ColumnName "=" ValueExpr ;

PredicateExpr    ::= ValueExpr ;
ValueExpr        ::= OrExpr ;
OrExpr           ::= AndExpr (("or" | "||") AndExpr)* ;
AndExpr          ::= EqualityExpr (("and" | "&&") EqualityExpr)* ;
EqualityExpr     ::= CompareExpr (("==" | "!=") CompareExpr)* ;
CompareExpr      ::= AddExpr (("<" | "<=" | ">" | ">=") AddExpr)* ;
AddExpr          ::= MulExpr (("+" | "-") MulExpr)* ;
MulExpr          ::= UnaryExpr (("*" | "/" | "%") UnaryExpr)* ;
UnaryExpr        ::= ("not" | "!" | "-") UnaryExpr | PrimaryExpr ;
PrimaryExpr      ::= Literal
                   | WindowExpr
                   | CallExpr
                   | ColumnRef
                   | "(" ValueExpr ")" ;
WindowExpr       ::= CallExpr "over" "(" WindowSpec ")" ;
WindowSpec       ::= PartitionClause? OrderClause? WindowFrame? ;
PartitionClause  ::= "partition_by" ColumnRef ("," ColumnRef)* ;
OrderClause      ::= "order_by" SortItem ("," SortItem)* ;
WindowFrame      ::= "rows" "between" FrameBound "and" FrameBound ;
FrameBound       ::= "unbounded_preceding"
                   | IntLiteral "preceding"
                   | "current_row"
                   | IntLiteral "following"
                   | "unbounded_following" ;
CallExpr         ::= Ident "(" ArgList? ")" ;
ArgList          ::= ValueExpr ("," ValueExpr)* ;
ColumnRef        ::= Ident | EscapedColumnRef ;
ColumnName       ::= Ident | EscapedColumnRef ;
Literal          ::= StringToken | NumberLiteral | BoolLiteral | "null" ;
BoolLiteral      ::= "true" | "false" ;
```

## 26. Appendix B: Example Suite

### 26.1 Top Regions

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount), avg_age = mean(customer_age)
  | sort total_revenue desc
  | limit 5
  | save "top_regions.csv"
```

### 26.2 Normalize CSV Headers

```pdl
load "raw_orders.csv"
  | select order_id = `Order ID`, customer_id = `Customer ID`, amount = `Amount`
  | filter amount > 0
  | save "orders.csv"
```

### 26.3 Clean And Deduplicate Orders

```pdl
load "orders_raw.csv"
  | filter lower(trim(status)) == "completed"
  | mutate
      net_amount = gross_amount - coalesce(discount, 0),
      region_channel = concat(upper(trim(region)), ":", lower(trim(channel))),
      priority = if_else(gross_amount >= 150, "high", "standard")
  | distinct order_id
  | select order_id, region_channel, net_amount, priority
  | sort order_id
```

The runnable repository example is `examples/orders_cleaned.pdl`.

### 26.4 Arrow Stream Output

```pdl
load "sales.parquet"
  | filter status == "completed"
  | group_by region
  | agg revenue = sum(amount)
```

Run:

```bash
pdl run sales_for_stream.pdl --stdout-format arrow-stream > sales.arrow
```

### 26.5 Join

```pdl
let customers =
  load "customers.csv"
  | select customer_id, segment

load "sales.csv"
  | filter status == "completed"
  | join customers on customer_id kind left
  | group_by segment
  | agg revenue = sum(amount), orders = count()
  | sort revenue desc
```

## 27. Appendix C: Implementation Checklist

Syntax:

- Lexer supports UTF-8, comments, string tokens, backtick column references,
  numbers, identifiers, operators, and spans.
- Parser supports resilient pipeline parsing.
- Parser supports `let` bindings, named `output` declarations, and main pipeline.
- Formatter emits stable leading-pipe style.

Semantics:

- Analyzer resolves bindings and columns.
- Analyzer tracks schema after every stage.
- Analyzer validates grouping and aggregation.
- Analyzer validates `pivot_longer`, `complete`, and named output declarations.
- Analyzer validates format clauses and stage options.
- Analyzer detects binding cycles.

Data and driver:

- Driver resolves paths and stdin/stdout descriptors.
- Driver infers formats by extension.
- Driver sniffs stdin when needed.
- Data crate reads CSV and Arrow IPC stream.
- Reference implementation reads and writes CSV, JSON Lines, Parquet, Arrow IPC
  file, and Arrow IPC stream formats where supported by host boundaries.

Execution and output:

- Exec crate emits deterministic CSV.
- Exec crate emits Arrow IPC streams.
- Exec crate evaluates named outputs in source order.
- CLI emits deterministic dry-run manifest and schema JSON.
- CLI keeps logs off stdout in data-output mode.

Editor and browser:

- `pdl lsp` serves diagnostics, completion, hover, formatting, and semantic tokens.
- VS Code extension is a thin LSP client.
- WASM exposes editor-service JSON ABI.
- WASM run JSON exposes named output tables.
- Monaco demo uses WASM and does not reimplement language logic.

Stream interop:

- PDL can stream Arrow IPC to stdout.
- PDL can save CSV for file-based consumers.
- Tests validate Arrow stream output with a consumer.

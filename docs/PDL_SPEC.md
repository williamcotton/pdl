# PDL Detailed Specification

Status: Draft 0.3.0
Audience: implementers, language designers, data engineers, runtime engineers, LSP authors, WASM host authors, VS Code extension authors, test authors, and Algraf users
Scope: standalone Unix-pipeline-style DSL for deterministic tabular data loading, transformation, aggregation, streaming, and materialization

## Current Reference Implementation Status

The current repository implementation is `0.3.0`.

This release keeps the existing CSV-backed language and runtime slice stable and
promotes diagnostics into a first-class compatibility surface. It implements the
`pdl` CLI commands `run`, `check`, and `version`; CSV file loading with header
rows; CSV file and stdout output; deterministic in-memory execution for `load`,
`filter`, `select`, `drop`, `rename`, `group_by`, `agg`, `sort`, `limit`, and
`save`; and the aggregate functions `count`, `sum`, `mean`, `min`, and `max`.
It also implements registered lettered diagnostic codes in `pdl-core`, a
`codes::*` registry, `related` spans and `help` diagnostic payload fields,
diagnostic catalog drift tests, `pdl lsp` with full-document sync, diagnostics,
completion, hover, formatting, semantic tokens, document symbols, and
same-document binding definition/reference/rename; and it ships a thin VS Code
client under `editors/vscode/`.

Version 0.3.0 does not yet implement Arrow IPC, Parquet, JSON Lines, stdin
loading, stream sniffing, configurable CSV dialect options, `mutate`, `join`,
`union`, `distinct`, window expressions, manifests, schema/plan subcommands,
CLI formatting, full LSP code actions or cross-document navigation, WASM entry
points, or browser demo support. Those features are tracked as deferred or
planned work in successor release plans such as `docs/V0_4_PLAN.md`.

## 0. Document Contract

This document specifies PDL, a standalone Pipeline Data Language.

PDL is not part of Algraf.

PDL is designed to pair well with Algraf by producing deterministic tabular data that Algraf can consume from ordinary files or streams.

The intended file extension is `.pdl`.

The intended command-line executable is named `pdl`.

The first reference implementation target is a Rust workspace organized like Algraf.

PDL is built around one core idea:

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age"
  | sort "total_revenue" desc
  | limit 5
  | save "top_regions.csv"
```

The pipeline reads left to right.

Each stage consumes a table.

Each stage produces a table, a stream, or an explicit output artifact.

PDL is intentionally close to Unix pipes.

It is not block-scoped like Algraf.

Algraf declares how data becomes marks in a visual space.

PDL declares how data becomes another table.

The two languages should compose through ordinary artifacts:

```bash
pdl run prep.pdl --stdout-format arrow-stream | algraf render chart.ag --stdin-format arrow-stream --output chart.svg
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
over a partition and order of rows. Window expressions are planned but not
implemented in version 0.3.0.

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

Algraf references in this document define interoperability context only.

They do not amend the Algraf specification.

An Algraf implementation does not need to read PDL.

A PDL implementation does not need to render SVG.

## 1. Executive Summary

PDL is a domain-specific language for tabular data preparation.

It uses a concise pipe syntax instead of nested blocks.

The smallest useful program loads data, transforms it, and writes it:

```pdl
load "orders.csv"
  | filter "amount" > 0
  | select "order_id", "region", "amount"
  | save "orders_clean.csv"
```

The `load` stage creates the initial table.

The `filter` stage keeps rows.

The `select` stage keeps and orders columns.

The `save` stage writes the final table.

PDL supports aggregation:

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age"
  | sort "total_revenue" desc
  | limit 5
  | save "top_regions.csv"
```

PDL supports named pipeline bindings for reuse and joins:

```pdl
let customers =
  load "customers.parquet"
  | select "customer_id", "segment"

load "sales.parquet"
  | filter "status" == "completed"
  | join customers on "customer_id" kind left
  | group_by "segment"
  | agg sum("amount") as "revenue"
  | save "segment_revenue.csv"
```

Named bindings are still pipeline expressions.

They are not task blocks.

They are evaluated only when referenced by the main pipeline or a selected output.

PDL supports stdout for Unix composition:

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue"
```

When run with stdout enabled, the final table can be streamed:

```bash
pdl run sales.pdl --stdout-format arrow-stream
```

This makes PDL a natural producer for Algraf charts that read stdin.

The preferred PDL to Algraf handoff is Arrow IPC streaming.

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

PDL MAY support an integrated PDL plus Algraf runner in a later project.

PDL MAY support package management in later versions.

PDL MAY support remote sources in later versions.

## 3. Non-Goals

PDL is not a general-purpose programming language.

PDL is not a charting language.

PDL is not an Algraf extension language.

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

True in-process zero-copy between PDL and Algraf is a future integrated-runner concern, not the PDL v0.1 language contract.

## 4. Core Concepts

### 4.1 Pipeline

A pipeline is an ordered expression connected by `|`.

The first stage MUST be `load` or a binding reference.

Every non-terminal transform stage consumes one table and produces one table.

Terminal output stages such as `save` produce an artifact and pass through the table unless otherwise specified.

This pass-through rule allows multiple saves:

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | save "completed.parquet"
  | group_by "region"
  | agg sum("amount") as "revenue"
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
filter "amount" > 0
group_by "region", "channel"
agg sum("amount") as "revenue"
sort "revenue" desc
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

Columns are usually referenced by double-quoted column names.

Example:

```pdl
filter "status" == "completed"
group_by "region"
agg sum("amount") as "total"
```

PDL uses context to distinguish column references from string literals.

In column-argument positions, a quoted token is a column reference.

Examples of column-argument positions:

- `select "a", "b"`
- `drop "a"`
- `group_by "region"`
- `sort "amount" desc`
- `sum("amount")`
- `join customers on "customer_id"`

In simple comparison predicates, the left operand SHOULD resolve as a column reference when it matches a known column.

The right operand SHOULD resolve as a literal unless explicitly wrapped with `col(...)`.

Example:

```pdl
filter "status" == "completed"
filter "region" == col("home_region")
```

When a schema is unavailable, the analyzer MAY treat quoted atoms in filter-left position as provisional column references.

Ambiguous quoted atoms SHOULD produce `W2002` with suggested `col("...")` or
`lit("...")` disambiguation.

### 4.5 Literal

Literals include strings, numbers, booleans, and null.

String literals use double quotes in value positions.

`lit("text")` forces a quoted value to be a string literal.

`col("name")` forces a quoted value to be a column reference.

Examples:

```pdl
filter col("status") == lit("completed")
mutate "label" = concat(col("region"), lit(": "), col("channel"))
```

The short form remains preferred for ordinary filters.

### 4.6 Binding

A binding names a pipeline expression.

Bindings use `let`.

Example:

```pdl
let completed =
  load "sales.parquet"
  | filter "status" == "completed"

completed
  | group_by "region"
  | agg sum("amount") as "revenue"
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
  | filter "amount" > 0
  | save "sales_clean.csv"
```

The pipeline loads a CSV file, filters rows, and writes a CSV file.

The output format is inferred from `.csv`.

### 5.2 Aggregation

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age"
  | sort "total_revenue" desc
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
  | select "Order ID" as "order_id", "Order Date" as "order_date", "Amount" as "amount"
  | save "orders_normalized.csv"
```

`select` may rename selected columns with `as`.

Column names with spaces are ordinary quoted column names.

### 5.4 Mutation

```pdl
load "orders.parquet"
  | mutate "net_amount" = "gross_amount" - "discount"
  | mutate "is_large" = "net_amount" >= 1000
  | save "orders_with_net.parquet"
```

`mutate` adds or replaces columns.

Assignments in one `mutate` stage are evaluated against the input schema in parallel.

Later stages see newly created columns.

### 5.5 Join

```pdl
let customers =
  load "customers.parquet"
  | select "customer_id", "segment"

load "sales.parquet"
  | join customers on "customer_id" kind left
  | group_by "segment"
  | agg sum("amount") as "revenue"
  | save "segment_revenue.csv"
```

`join` references a binding or inline `load` source.

Version 0.1 SHOULD support joins against named bindings.

Inline join pipelines MAY be deferred.

### 5.6 Stdin And Stdout

```pdl
load stdin
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "revenue"
```

Run:

```bash
cat sales.arrow | pdl run prep.pdl --stdin-format arrow-stream --stdout-format arrow-stream
```

If `--stdin-format` is omitted, the driver SHOULD sniff the stream.

If `--stdout-format` is omitted and stdout is piped, the CLI SHOULD default to `arrow-stream`.

If stdout is a terminal, the CLI MAY default to a human-readable preview unless `--stdout-format` is supplied.

### 5.7 Algraf Handoff

PDL source:

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "revenue"
```

Algraf source:

```ag
Chart(data: stdin) {
    Space(region * revenue) {
        Bar(fill: region)
    }
}
```

Command:

```bash
pdl run prep.pdl --stdout-format arrow-stream | algraf render chart.ag --stdin-format arrow-stream --output chart.svg
```

The `.pdl` file owns data preparation.

The `.ag` file owns visual mapping.

PDL MUST NOT mutate the `.ag` file.

### 5.8 Explicit Format Override

```pdl
load stdin format "csv"
  | filter "status" == "completed"
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

Identifiers name bindings and bare stage selectors.

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
- `let`
- `as`
- `on`
- `kind`
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

Reserved words MUST NOT be used as binding identifiers.

Column names may match reserved words because quoted column references are strings.

Planned window expression syntax uses additional clause words:

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

These words are not reserved by the version 0.3.0 implementation until window
syntax is implemented.

### 6.6 Quoted Tokens

Double-quoted tokens are used for both column names and string literals.

Context determines interpretation.

The escape sequences are:

- `\"`
- `\\`
- `\n`
- `\r`
- `\t`
- `\u{HEX}`

Unterminated quoted tokens MUST produce `E0002`.

Invalid escapes MUST produce `E0004`.

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
Program       ::= Trivia* BindingDecl* PipelineExpr Trivia* EOF ;
BindingDecl   ::= "let" Ident "=" PipelineExpr ;
PipelineExpr  ::= PipelineStart PipelineTail* ;
PipelineStart ::= LoadStage | Ident ;
PipelineTail  ::= "|" Stage ;
Stage         ::= TransformStage | SaveStage ;
```

A file contains zero or more `let` bindings followed by one main pipeline expression.

The main pipeline expression is the default run target.

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
                 | DistinctStage ;
```

### 7.5 Filter

```ebnf
FilterStage   ::= "filter" PredicateExpr ;
```

The predicate expression must evaluate to boolean or nullable boolean.

Rows are kept only when the predicate is true.

### 7.6 Select, Drop, Rename

```ebnf
SelectStage   ::= "select" SelectItem ("," SelectItem)* ;
SelectItem    ::= ColumnRef ("as" ColumnName)? ;
DropStage     ::= "drop" ColumnRef ("," ColumnRef)* ;
RenameStage   ::= "rename" RenameItem ("," RenameItem)* ;
RenameItem    ::= ColumnRef "as" ColumnName ;
```

`select` keeps columns in listed order.

`drop` removes columns.

`rename` preserves order.

### 7.7 Mutate

```ebnf
MutateStage   ::= "mutate" Assignment ("," Assignment)* ;
Assignment    ::= ColumnName "=" ValueExpr ;
```

The left side names a new or replaced column.

### 7.8 Group And Aggregate

```ebnf
GroupByStage  ::= "group_by" ColumnRef ("," ColumnRef)* ;
AggStage      ::= "agg" AggItem ("," AggItem)* ;
AggItem       ::= AggCall "as" ColumnName ;
AggCall       ::= Ident "(" AggArgList? ")" ;
AggArgList    ::= ValueExpr ("," ValueExpr)* ;
```

`agg` consumes active group state.

If there is no active group state, `agg` aggregates the whole table into one row.

### 7.9 Sort And Limit

```ebnf
SortStage     ::= "sort" SortItem ("," SortItem)* ;
SortItem      ::= ColumnRef SortDirection? NullsOrder? ;
SortDirection ::= "asc" | "desc" ;
NullsOrder    ::= "nulls_first" | "nulls_last" ;
LimitStage    ::= "limit" IntLiteral ;
```

`sort` is stable.

`limit` keeps the first `n` rows in current order.

### 7.10 Join

```ebnf
JoinStage     ::= "join" JoinSource JoinOn JoinKind? ;
JoinSource    ::= Ident ;
JoinOn        ::= "on" ColumnRef
                | "on" "(" ColumnRef "," ColumnRef ")" ;
JoinKind      ::= "kind" JoinKindName ;
JoinKindName  ::= "inner" | "left" | "right" | "full" | "semi" | "anti" ;
```

`join customers on "customer_id"` joins the current table to the `customers` binding.

`on ("left_id", "right_id")` joins differently named keys.

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

`distinct` removes duplicate rows.

### 7.12 Expressions

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
                | ColumnToken
                | Ident
                | WindowExpr
                | CallExpr
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
```

Comparison chaining is not supported.

`"a" < "b" < "c"` MUST produce `E1408` or a type error with help suggesting
`"a" < "b" and "b" < "c"`.

Window expressions are planned syntax and are not implemented in version 0.3.0.

Until implemented, parsers MAY recover with `E1211` or ordinary parse diagnostics
when they encounter `over`.

### 7.13 Error Recovery

The parser MUST recover from malformed pipelines.

Recovery points include:

- `|`
- newline followed by a stage keyword
- `let`
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

The main pipeline can reference earlier bindings.

Bindings can reference earlier bindings.

Forward binding references MAY be allowed if the analyzer resolves the whole file before execution.

If forward references are allowed, cycles MUST still be rejected.

### 8.2 Pipeline Scope

Each stage sees the schema produced by the previous stage.

Column references resolve against that current schema.

Columns introduced by `mutate`, `select as`, `rename`, or `agg as` are available to later stages.

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

Column names may match reserved keywords.

Column names may match binding names because column references are quoted and binding references are bare identifiers.

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
- configurable delimiter
- configurable quote character
- configurable null tokens

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

Parquet output MAY be supported in version 0.1.

Unsupported Parquet logical types MUST produce `E1808`.

Parquet reading SHOULD push down projection and filters where practical, but optimization MUST NOT change semantics.

### 10.4 Arrow IPC File

Arrow IPC file format SHOULD be supported.

Arrow IPC files commonly start with magic bytes `ARROW1`.

The reader MUST validate Arrow metadata.

The writer SHOULD emit deterministic schema metadata.

### 10.5 Arrow IPC Stream

Arrow IPC stream format MUST be supported for stdout output.

Arrow IPC stream input SHOULD be supported for stdin.

Arrow streams begin with a continuation marker and schema message.

The runtime SHOULD read and write record batches without unnecessary conversion.

The language does not require true zero-copy across OS processes.

### 10.6 JSON Lines

JSON Lines support MAY be implemented.

Each non-empty line must be a JSON object.

Nested objects are not flattened by default.

Schema inference for JSON Lines MUST be deterministic if implemented.

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

`filter` may refine nullability after checks such as `"col" != null`.

### 11.3 Select

`select` keeps columns in listed order.

`select "a" as "b"` renames a selected column.

Unknown selected columns MUST produce `E1005`.

Duplicate output column names MUST produce `E1207`.

### 11.4 Drop

`drop` removes listed columns.

`drop` preserves order of remaining columns.

Dropping an unknown column MUST produce `E1005`.

Dropping all columns is legal but SHOULD produce `W2003`.

### 11.5 Rename

`rename "old" as "new"` renames columns.

Rename preserves column order.

Renaming to an existing column MUST produce `E1207` unless overwrite behavior is explicitly supported.

### 11.6 Mutate

`mutate "name" = expression` adds or replaces columns.

Assignments in one stage are parallel.

Later assignments in the same stage MUST NOT see earlier assignments unless a future sequential mode is introduced.

Replacing an existing column preserves its position.

New columns append in assignment order.

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

Every aggregate item MUST use `as`.

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

`join binding on "key"` joins the current table with a named binding.

Supported kinds:

- `inner`
- `left`
- `right`
- `full`
- `semi`
- `anti`

Default kind is `inner`.

Join key types must be compatible.

Duplicate non-key output column names MUST be resolved or diagnosed.

The default collision policy SHOULD suffix right-side columns with `_right`.

If a suffix creates another collision, the analyzer MUST produce `E1207` unless
an explicit suffix option is introduced.

### 11.12 Union

`union binding` combines rows from the current table and a named binding.

Default behavior SHOULD align columns by name.

Schemas must be compatible.

`distinct true` removes duplicate rows.

Union output ordering preserves left rows followed by right rows unless distinct handling requires deterministic de-duplication.

### 11.13 Distinct

`distinct` removes duplicate rows using all columns.

`distinct "a", "b"` removes duplicates using selected key columns.

The first row in current order is retained.

Output order follows retained row order.

### 11.14 Window Expressions (Planned)

Window expressions are planned row expressions that add or replace columns
without changing row count.

They are intended primarily for `mutate`:

```pdl
load "sales.parquet"
  | mutate "region_revenue" = sum("amount") over (partition_by "region")
  | mutate "region_rank" = dense_rank() over (partition_by "region" order_by "amount" desc)
```

Window expressions MUST NOT use active `group_by` state.

`group_by` remains state for `agg` only; a window partition is always declared
explicitly with `partition_by`.

Window expressions preserve the current row order.

`partition_by` columns must exist.

`order_by` uses the same sort item syntax and null ordering rules as `sort`.

If `partition_by` is omitted, the whole input table is one partition.

If `order_by` is omitted, aggregate window functions operate over the current
partition order.

Ranking and offset window functions require `order_by`.

Assignments in a `mutate` stage containing window expressions remain parallel:
one assignment MUST NOT see another assignment from the same stage.

Window execution MAY require materializing the current table or partition.

## 12. Expressions And Functions

### 12.1 Expression Contexts

PDL has row expression context, aggregate context, path context, and format context.

Row expressions can reference columns.

Aggregate expressions can reference aggregate functions and group keys.

Window expressions are a planned row-expression form. They do not introduce
aggregate context, and they are not valid inside `agg`.

Path context accepts string literals and future path functions.

Format context accepts canonical format names.

### 12.2 Column And Literal Disambiguation

Because PDL uses double quotes for the concise column syntax, expression analysis MUST be deterministic.

Rules:

1. In declared column positions, quoted tokens are column references.
2. In aggregate function arguments, quoted tokens are column references unless wrapped by `lit`.
3. In the left operand of a simple comparison predicate, quoted tokens are column references when matching or provisionally matching the current schema.
4. In the right operand of a simple comparison predicate, quoted tokens are string literals unless wrapped by `col`.
5. `col("name")` always means column reference.
6. `lit("value")` always means string literal.

Implementations SHOULD emit helpful diagnostics when a quoted token could plausibly mean the other interpretation.

### 12.3 Scalar Functions

Recommended scalar functions:

- `col(name)`
- `lit(value)`
- `is_null(value)`
- `not_null(value)`
- `coalesce(values...)`
- `concat(values...)`
- `lower(value)`
- `upper(value)`
- `trim(value)`
- `contains(value, pattern)`
- `starts_with(value, prefix)`
- `ends_with(value, suffix)`
- `date(value)`
- `datetime(value)`
- `date_floor(value, unit)`
- `year(value)`
- `month(value)`
- `day(value)`

Unknown functions MUST produce `E1401`.

Function calls MUST be pure.

### 12.4 Aggregate Functions

Required aggregate functions:

- `count()`
- `count(column)`
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

`sum`, `mean`, `min`, and `max` ignore null values.

Aggregating an empty group returns null except for `count`, which returns zero.

### 12.5 Window Functions (Planned)

Window function syntax is planned and not implemented in version 0.3.0.

Window functions use ordinary function-call syntax followed by an `over` clause.

Examples:

```pdl
load "orders.csv"
  | mutate "customer_total" = sum("amount") over (partition_by "customer_id")
  | mutate "running_total" =
      sum("amount") over (
        partition_by "customer_id"
        order_by "order_date" asc
        rows between unbounded_preceding and current_row
      )
```

```pdl
load "orders.csv"
  | mutate "rn" =
      row_number() over (
        partition_by "customer_id"
        order_by "order_date" desc, "order_id" asc
      )
  | filter "rn" == 1
```

Planned ranking and offset functions:

- `row_number()`
- `rank()`
- `dense_rank()`
- `lag(column)`
- `lead(column)`

Planned aggregate-style window functions:

- `count()`
- `count(column)`
- `sum(column)`
- `mean(column)`
- `min(column)`
- `max(column)`
- `first(column)`
- `last(column)`

Aggregate-style window functions without `order_by` default to the whole
partition.

Aggregate-style window functions with `order_by` default to:

```pdl
rows between unbounded_preceding and current_row
```

Ranking and offset functions ignore frames and SHOULD reject explicit frames
unless a future version gives them frame semantics.

For `rank` and `dense_rank`, peer rows are rows with equal `order_by` values.

For `row_number`, rows with equal `order_by` values use the current stable row
order as the deterministic tie-breaker. Users SHOULD add explicit tie-breaker
columns when they need durable rankings independent of input order.

Invalid window specifications MUST produce `E1409` once window syntax is
implemented.

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

Invalid window specification MUST produce `E1409` once window syntax is
implemented.

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

`pdl run file.pdl` parses, analyzes, plans, and executes the main pipeline.

Recommended options:

- `--stdin-format <format>`
- `--stdout-format <format>`
- `--output <path>`
- `--manifest <path>`
- `--dry-run`
- `--strict`
- `--permissive`

If the pipeline has `save` stages, those stages write their artifacts.

If stdout output is requested, the final table is written to stdout.

Operational logs MUST go to stderr so stdout remains a clean data stream.

### 14.3 pdl check

`pdl check file.pdl` parses and analyzes without executing the full pipeline.

It MUST report syntax and semantic diagnostics.

It SHOULD infer schemas from file metadata where cheap.

It SHOULD avoid reading full data files by default.

It MUST exit non-zero on errors.

### 14.4 pdl schema

`pdl schema file.pdl` prints inferred schema for the main pipeline.

`pdl schema file.pdl --binding name` prints schema for a binding.

JSON output SHOULD be supported.

### 14.5 pdl plan

`pdl plan file.pdl` prints the execution plan.

It MUST not write output artifacts.

It SHOULD show source reads, transform stages, format decisions, and sinks.

JSON output SHOULD be supported.

### 14.6 pdl fmt

`pdl fmt file.pdl` formats source.

`pdl fmt --check file.pdl` checks formatting without writing.

The formatter MUST preserve semantics.

### 14.7 pdl lsp

`pdl lsp` runs the language server over standard input and standard output.

The LSP backend MUST share parser and analyzer code with CLI.

### 14.8 Exit Codes

Exit code `0` means success.

Exit code `1` means diagnostics or runtime failure.

Exit code `2` means CLI usage error.

Additional exit codes MAY be defined.

### 14.9 Stdout Discipline

When stdout is used for data, all human-readable logs MUST go to stderr.

Diagnostics in human-readable form MUST go to stderr.

JSON diagnostics MAY go to stdout only for commands whose output is diagnostics rather than data, such as `check --json`.

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

`filter`, `select`, `drop`, `rename`, and simple `mutate` can stream.

`group_by` plus `agg`, `sort`, `join`, `distinct`, window expressions, and
some `union` modes may require materialization.

The plan SHOULD identify blocking stages.

### 15.5 Failure Semantics

Static errors stop execution.

Runtime source errors stop dependent stages.

Write errors fail the run.

In permissive mode, row-level parse errors MAY be collected while execution continues.

Strict mode MUST fail on row-level parse errors.

### 15.6 Manifests

The runtime SHOULD emit a run manifest when requested.

Manifest fields SHOULD include:

- PDL source path
- implementation version
- input sources
- detected formats
- output artifacts
- final schema
- row counts where known
- content hashes where computed
- diagnostics
- Algraf interop hints when stdout format is Arrow IPC

Manifest JSON MUST be deterministic.

## 16. Algraf Interoperability

### 16.1 Interop Principle

PDL and Algraf are separate languages.

PDL prepares tables.

Algraf renders charts.

The preferred interop boundary is Arrow IPC streaming over stdout/stdin.

CSV files are the portable fallback.

### 16.2 Unix Arrow Streaming

PDL SHOULD support:

```bash
pdl run prep.pdl --stdout-format arrow-stream
```

Algraf-compatible workflows can pipe this stream:

```bash
pdl run prep.pdl --stdout-format arrow-stream | algraf render chart.ag --stdin-format arrow-stream --output chart.svg
```

PDL's responsibility is to produce a valid Arrow IPC stream.

Algraf's responsibility is to consume stdin if it supports that mode.

This PDL specification does not require Algraf to implement new flags.

### 16.3 Stdin Format Sniffing For Consumers

PDL recommends that consumers reading unknown stdin support sniffing plus explicit override.

For PDL itself, this is normative for `load stdin`.

For Algraf, this is interop guidance only.

### 16.4 File-Based Handoff

PDL can materialize a file:

```pdl
load "sales.parquet"
  | group_by "region"
  | agg sum("amount") as "revenue"
  | save "build/revenue.csv"
```

Algraf can reference that file:

```ag
Chart(data: "build/revenue.csv") {
    Space(region * revenue) {
        Bar(fill: region)
    }
}
```

File-based handoff is slower than Arrow streaming but simpler to inspect and cache.

### 16.5 Browser Handoff

In browser hosts, PDL WASM SHOULD be able to return Arrow bytes as a `Uint8Array` through the host ABI.

A host MAY pass those bytes to an Algraf WASM runtime if one is loaded.

PDL WASM MUST NOT invoke native Algraf.

PDL WASM MUST NOT assume Algraf WASM is present.

### 16.6 Integrated Runner

A future integrated runner MAY link PDL and Algraf crates in one Rust process.

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

The current `0.3.0` LSP implementation provides diagnostics,
completion, hover, formatting, semantic tokens, document symbols, and
same-document binding go-to-definition, references, and rename. Code actions and
cross-document navigation remain deferred.

The current formatter withholds edits for documents containing comments because
the current parser does not preserve comment trivia. This avoids changing source
text in ways the syntax tree cannot faithfully represent yet.

### 17.2 Completion

Completion SHOULD support:

- stage names after `|`
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

### 17.4 Formatting

The formatter SHOULD use:

- one stage per line for multi-stage pipelines
- two spaces before leading `|`
- spaces around binary operators
- comma plus space between items

Example formatted style:

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue"
  | save "out.csv"
```

### 17.5 VS Code Client

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

The language configuration is only for editor behaviors such as brackets, comments, indentation, and word patterns.

The extension SHOULD activate on `.pdl` files and the `pdl` language id.

The extension SHOULD register commands only for client wiring, such as restart server or show output channel.

The extension package version SHOULD align with workspace release version.

When PDL syntax changes, `syntaxes/pdl.tmLanguage.json` and `language-configuration.json` SHOULD be updated in the same change.

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
- formatting
- schema inspection
- plan inspection
- bounded in-memory execution
- Arrow IPC byte output
- editor-service requests for Monaco

Editor-service requests SHOULD use LSP-shaped positions and results.

The ABI boundary uses UTF-16 positions.

Internal spans remain byte offsets.

### 18.4 Monaco Host

The reference repository SHOULD include `demo/`.

The demo host SHOULD use Monaco.

The demo host MUST call the WASM editor-service ABI for language features.

The demo host MUST NOT implement a separate PDL parser or analyzer.

The demo MAY show generated Arrow, CSV, schema, manifest, and Algraf handoff examples.

## 19. Rust Crate Architecture

### 19.1 Workspace Layout

PDL MUST follow the same general crate architecture as Algraf.

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

The standalone PDL repository SHOULD copy Algraf's Cargo workspace structure as
a one-to-one scaffold, replacing `algraf` package names with `pdl` package names
and replacing the graphics render crate with the data-pipeline execution crate.

The expected manifest mapping is:

| Algraf manifest | PDL manifest |
| --- | --- |
| `Cargo.toml` | `Cargo.toml` |
| `crates/algraf-core/Cargo.toml` | `crates/pdl-core/Cargo.toml` |
| `crates/algraf-syntax/Cargo.toml` | `crates/pdl-syntax/Cargo.toml` |
| `crates/algraf-data/Cargo.toml` | `crates/pdl-data/Cargo.toml` |
| `crates/algraf-semantics/Cargo.toml` | `crates/pdl-semantics/Cargo.toml` |
| `crates/algraf-driver/Cargo.toml` | `crates/pdl-driver/Cargo.toml` |
| `crates/algraf-render/Cargo.toml` | `crates/pdl-exec/Cargo.toml` |
| `crates/algraf-editor-services/Cargo.toml` | `crates/pdl-editor-services/Cargo.toml` |
| `crates/algraf-lsp/Cargo.toml` | `crates/pdl-lsp/Cargo.toml` |
| `crates/algraf-cli/Cargo.toml` | `crates/pdl-cli/Cargo.toml` |
| `crates/algraf-wasm/Cargo.toml` | `crates/pdl-wasm/Cargo.toml` |

All PDL crates MUST inherit package version, edition, license, repository, and
Rust version from `[workspace.package]`, matching the Algraf manifest pattern.

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
    "crates/pdl-wasm",
]

[workspace.package]
version = "0.3.0"
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
    "lazy",
    "csv",
    "ipc",
    "ipc_streaming",
    "parquet",
    "json",
    "temporal",
    "dtype-slim",
    "fmt",
    "strings",
    "regex",
    "rank",
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

[dev-dependencies]
pdl-syntax = { workspace = true }
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

Graphics-only Algraf dependencies such as SVG rasterization, projection, and
geometry-source crates MUST NOT be copied into PDL unless a later PDL feature
requires them and the specification documents that requirement.

### 19.3 Module Boundaries

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

`pdl-exec` is the PDL analog of Algraf's render-stage crate.

It executes the analyzed pipeline and turns internal table results into deterministic external artifacts.

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

PDL SHOULD keep the same general third-party dependency stack as Algraf wherever
the domains overlap: Cargo workspace conventions, CLI parsing, resilient syntax
trees, serde-based JSON, CSV/Arrow/Parquet data handling, LSP transport, async
runtime, stable ordering, diagnostics, and snapshot/test helpers should use the
same crate families unless the PDL specification documents a deliberate
substitution.

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

### 19.6 Implementation Patterns From Algraf

The standalone PDL repository SHOULD preserve Algraf's implementation patterns
where they support deterministic analysis, resilient editor behavior, and shared
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
- `as` aliases
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

`join` MUST validate key compatibility and produce deterministic column
collision diagnostics before planning execution.

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
- schema and bounded metadata loading
- semantic analysis
- planning
- execution preview where requested

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
- `E1901`-`E1999`: Algraf interop errors
- `W2001`-`W2099`: author-facing warnings
- `H3001`-`H3099`: author-facing hints
- `R4001`-`R4099`: implementation-oriented runtime/internal diagnostics

The implementation MUST NOT emit an unregistered code.

When a condition could fit multiple codes, the most specific source-facing code
wins. For example, an unknown column in `select` is `E1005`; it is not a generic
stage-argument error.

### 20.2 Syntax Diagnostics

`E0001` unexpected token.

`E0002` unterminated quoted token.

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

`E0014` expected alias after `as`.

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

### 20.3 Binding, Scope, Column, And Schema Diagnostics

`E1001` duplicate binding name.

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

`E1205` duplicate stage option.

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

`E1226` window syntax is not enabled.

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

`E1417` aggregate item requires `as`.

### 20.7 Planning Diagnostics

`E1501` binding dependency cycle.

`E1502` no runnable main pipeline.

`E1503` selected binding not found.

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

### 20.11 Algraf Interop Diagnostics

`E1901` Algraf handoff format unsupported.

`E1902` Algraf interop manifest failed.

`E1903` Algraf handoff stream format conflict.

`E1904` Algraf handoff path invalid.

`E1905` Algraf consumer format could not be inferred.

### 20.12 Warning Diagnostics

`W2001` active grouping state was not consumed by `agg`.

`W2002` ambiguous quoted token.

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

### 20.13 Hint Diagnostics

`H3001` use `col(...)` or `lit(...)` to disambiguate a quoted token.

`H3002` add `agg` after `group_by`.

`H3003` add `sort` before `limit` for deterministic top-N output.

`H3004` add an explicit `format` clause.

`H3005` add a tie-breaker column to window `order_by`.

`H3006` use `arrow-stream` for Algraf handoff.

`H3007` use `--stdout-format` when piping data.

`H3008` add `as` to avoid an output column collision.

`H3009` use the canonical lowercase format name.

`H3010` add an explicit overwrite option when replacing files.

### 20.14 Internal Runtime Diagnostics

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
- Algraf interop tests
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

Pipeline-to-Algraf interop tests MAY use a fake consumer that validates Arrow IPC bytes.

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

PDL source does not require an explicit version declaration in draft 0.3.0.

The implementation SHOULD report supported language version.

Patch releases SHOULD preserve syntax and semantics.

Minor releases MAY add stages, functions, formats, and diagnostics.

Breaking changes require a major version after 1.0.

Diagnostic codes MUST remain stable after release.

Manifest schemas MUST include a manifest version.

## 25. Appendix A: Complete EBNF Draft

```ebnf
Program          ::= Trivia* BindingDecl* PipelineExpr Trivia* EOF ;
BindingDecl      ::= "let" Ident "=" PipelineExpr ;
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
                   | SaveStage ;

LoadStage        ::= "load" SourceRef FormatClause? ;
SourceRef        ::= StringToken | "stdin" | "-" ;
SaveStage        ::= "save" SinkRef FormatClause? SaveOptions? ;
SinkRef          ::= StringToken | "stdout" | "-" ;
FormatClause     ::= "format" FormatName ;
FormatName       ::= StringToken | Ident ;

FilterStage      ::= "filter" PredicateExpr ;
SelectStage      ::= "select" SelectItem ("," SelectItem)* ;
SelectItem       ::= ColumnRef ("as" ColumnName)? ;
DropStage        ::= "drop" ColumnRef ("," ColumnRef)* ;
RenameStage      ::= "rename" RenameItem ("," RenameItem)* ;
RenameItem       ::= ColumnRef "as" ColumnName ;
MutateStage      ::= "mutate" Assignment ("," Assignment)* ;
Assignment       ::= ColumnName "=" ValueExpr ;
GroupByStage     ::= "group_by" ColumnRef ("," ColumnRef)* ;
AggStage         ::= "agg" AggItem ("," AggItem)* ;
AggItem          ::= AggCall "as" ColumnName ;
AggCall          ::= Ident "(" ArgList? ")" ;
SortStage        ::= "sort" SortItem ("," SortItem)* ;
SortItem         ::= ColumnRef SortDirection? NullsOrder? ;
SortDirection    ::= "asc" | "desc" ;
NullsOrder       ::= "nulls_first" | "nulls_last" ;
LimitStage       ::= "limit" IntLiteral ;
JoinStage        ::= "join" JoinSource JoinOn JoinKind? ;
JoinSource       ::= Ident ;
JoinOn           ::= "on" ColumnRef | "on" "(" ColumnRef "," ColumnRef ")" ;
JoinKind         ::= "kind" JoinKindName ;
JoinKindName     ::= "inner" | "left" | "right" | "full" | "semi" | "anti" ;
UnionStage       ::= "union" Ident UnionOptions? ;
UnionOptions     ::= UnionOption* ;
UnionOption      ::= "by_name" BoolLiteral | "distinct" BoolLiteral ;
DistinctStage    ::= "distinct" ColumnRefList? ;
ColumnRefList    ::= ColumnRef ("," ColumnRef)* ;

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
                   | ColumnToken
                   | Ident
                   | WindowExpr
                   | CallExpr
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
ColumnRef        ::= StringToken | CallExpr ;
ColumnName       ::= StringToken ;
Literal          ::= StringToken | NumberLiteral | BoolLiteral | "null" ;
BoolLiteral      ::= "true" | "false" ;
```

## 26. Appendix B: Example Suite

### 26.1 Top Regions

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age"
  | sort "total_revenue" desc
  | limit 5
  | save "top_regions.csv"
```

### 26.2 Normalize CSV Headers

```pdl
load "raw_orders.csv"
  | select "Order ID" as "order_id", "Customer ID" as "customer_id", "Amount" as "amount"
  | filter "amount" > 0
  | save "orders.csv"
```

### 26.3 Arrow Stream To Algraf

```pdl
load "sales.parquet"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "revenue"
```

Run:

```bash
pdl run sales_for_chart.pdl --stdout-format arrow-stream | algraf render chart.ag --stdin-format arrow-stream --output chart.svg
```

### 26.4 Join

```pdl
let customers =
  load "customers.parquet"
  | select "customer_id", "segment"

load "orders.parquet"
  | join customers on "customer_id" kind left
  | group_by "segment"
  | agg sum("amount") as "revenue", count() as "orders"
  | save "segment_summary.csv"
```

## 27. Appendix C: Implementation Checklist

Syntax:

- Lexer supports UTF-8, comments, quoted tokens, numbers, identifiers, operators, and spans.
- Parser supports resilient pipeline parsing.
- Parser supports `let` bindings and main pipeline.
- Formatter emits stable leading-pipe style.

Semantics:

- Analyzer resolves bindings and columns.
- Analyzer tracks schema after every stage.
- Analyzer validates grouping and aggregation.
- Analyzer validates format clauses and stage options.
- Analyzer detects binding cycles.

Data and driver:

- Driver resolves paths and stdin/stdout descriptors.
- Driver infers formats by extension.
- Driver sniffs stdin when needed.
- Data crate reads CSV and Arrow IPC stream.
- Reference implementation reads Parquet.

Execution and output:

- Exec crate emits deterministic CSV.
- Exec crate emits Arrow IPC streams.
- Exec crate emits manifests and schema JSON.
- CLI keeps logs off stdout in data-output mode.

Editor and browser:

- `pdl lsp` serves diagnostics, completion, hover, formatting, and semantic tokens.
- VS Code extension is a thin LSP client.
- WASM exposes editor-service JSON ABI.
- Monaco demo uses WASM and does not reimplement language logic.

Algraf interop:

- PDL can stream Arrow IPC to stdout.
- PDL can save CSV for file-based Algraf use.
- Tests validate Arrow stream output with a consumer.
- PDL does not mutate `.ag` source.

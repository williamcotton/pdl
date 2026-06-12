# PDL Language Reference

PDL is a deterministic tabular data preparation DSL. Files use the `.pdl`
extension. A PDL program loads table data, applies pipeline stages, and writes a
table to stdout or to files.

## Read This First

- PDL is not Python, SQL, shell, or a general scripting language.
- Do not invent loops, user-defined scalar functions, embedded code blocks,
  network fetches, package imports, or shell execution. They are not PDL.
- A pipeline starts with `load "path"` or with a named table binding, then each
  following stage starts with `|`.
- Use commas between multi-line stage items.
- Prefer small, explicit stages over one dense expression.
- Use `pdl check file.pdl` for diagnostics before running.
- Use `pdl fmt file.pdl` to rewrite source to canonical style. Use
  `pdl fmt file.pdl --check` when you only want to verify formatting.
- Use `pdl run file.pdl --stdout-format csv --dry-run` for quick stdout checks
  when a pipeline should emit CSV.
- Human diagnostics and logs go to stderr. Stdout is data.

## Minimal Example

```pdl
load "sales.csv"
  | filter status == "completed"
  | group_by region
  | agg
      total_revenue = sum(amount),
      avg_age = mean(customer_age),
      orders = count()
  | sort total_revenue desc
  | limit 5
```

## Comments

```pdl
// Single-line comment
/* Block comment */
```

Avoid non-ASCII decoration in comments. Keep comments plain and sparse.

## Pipeline Shape

The canonical style is a leading-pipe pipeline:

```pdl
load "input.csv"
  | filter amount > 0
  | select order_id, region, amount
  | save "clean.csv"
```

Each stage receives the table from the previous stage and returns a table for
the next stage.

## Loading Data

```pdl
load "orders.csv"
load "orders.tsv"
load "orders.jsonl"
load "orders.parquet"
load "orders.arrow"
load "orders.arrow-stream"
load stdin
```

When reading stdin, pass the format at the CLI when the format is not obvious:

```bash
pdl run prep.pdl --stdin-format csv --stdout-format arrow-stream
```

Supported common formats include CSV, TSV, JSON Lines, Parquet, Arrow IPC file,
and Arrow IPC stream. Prefer Arrow IPC stream for typed Unix-pipe handoff.

## Saving Data

```pdl
load "orders.csv"
  | filter status == "completed"
  | save "completed.parquet"
```

`save` writes a side artifact. If it is not terminal, the table continues down
the pipeline.

## Bindings

Use `let` for reusable table bindings.

```pdl
let completed =
  load "orders.csv"
    | filter status == "completed"

completed
  | group_by region
  | agg revenue = sum(amount)
```

Bindings are tables, not mutable variables. Do not assign scalar values with
`let`.

## Named Outputs

PDL can declare named outputs when a program needs multiple result tables.

```pdl
output regional =
  load "sales.csv"
    | group_by region
    | agg revenue = sum(amount)

output customers =
  load "sales.csv"
    | group_by customer_id
    | agg revenue = sum(amount)
```

Named outputs execute in source order. Do not try to concatenate unrelated
tables to one stdout stream.

Named outputs are not table bindings. If a later output needs to reuse an
earlier result, factor the shared table into `let` first:

```pdl
let monthly =
  load "commits.csv"
    | group_by month
    | agg lines = sum(lines_changed)

output monthly_lines =
  monthly
    | save "monthly_lines.csv"

output monthly_ranked =
  monthly
    | sort lines desc
    | save "monthly_ranked.csv"
```

When a PDL file contains multiple `output` declarations that each `save` their
own CSV, run it as `pdl run file.pdl`. Do not add `--stdout-format` unless the
program is intended to emit one stdout table.

## Core Stages

Complete shipped transform stage list:

```text
filter
select
drop
rename
mutate
group_by
agg
sort
limit
join
union
distinct
pivot_longer
complete
save
```

`load` starts a pipeline. `save` is a sink stage that may also pass the table
through when it is not terminal.

### filter

Keep rows where an expression is true.

```pdl
load "orders.csv"
  | filter status == "completed" and amount > 0
```

### select

Keep and order columns.

```pdl
load "orders.csv"
  | select order_id, region, amount
```

Rename while selecting with `new_name = existing_column`:

```pdl
load "orders.csv"
  | select id = order_id, region, amount
```

### drop

Remove columns.

```pdl
load "orders.csv"
  | drop internal_note, raw_payload
```

### rename

Rename columns.

```pdl
load "orders.csv"
  | rename
      id = order_id,
      region = customer_region
```

### mutate

Create or replace columns.

```pdl
load "orders.csv"
  | mutate
      net_amount = amount - discount,
      label = region + ":" + channel
```

### group_by and agg

Group rows, then aggregate.

```pdl
load "orders.csv"
  | group_by region, channel
  | agg
      revenue = sum(amount),
      orders = count(),
      avg_amount = mean(amount)
```

Common aggregate functions include `count()`, `sum(col)`, `mean(col)`,
`min(col)`, `max(col)`, and `count_distinct(col)`.

### sort and limit

```pdl
load "orders.csv"
  | sort revenue desc, region asc
  | limit 10
```

Sort directions are `asc` and `desc`. Null ordering options are `nulls_first`
and `nulls_last`.

### join

Join the current table to a binding.

```pdl
let customers =
  load "customers.csv"

load "orders.csv"
  | join customers on customer_id kind left
```

Common join kinds are `inner`, `left`, `right`, `full`, `semi`, and `anti`.
For different column names, use pair syntax:

```pdl
load "orders.csv"
  | join customers on (customer_id, id) kind left
```

Composite keys are comma-separated:

```pdl
load "orders.csv"
  | join prices on sku, region kind left
  | join products on (sku, product_sku), (region, market) kind left
```

### union

Append another table.

```pdl
let day2 =
  load "daily_orders_2026_02_02.csv"

load "daily_orders_2026_02_01.csv"
  | union day2 by_name true distinct true
```

Use `by_name true` when columns should align by name rather than by position.

### distinct

Keep the first row for each unique full row or key tuple.

```pdl
load "orders.csv"
  | distinct

load "orders.csv"
  | distinct order_id, line_id
```

Without a column list, `distinct` compares all columns. With a column list, it
compares only the listed key columns and keeps the first row for each key tuple.

### pivot_longer

Turn multiple value columns into name/value rows.

```pdl
load "wide_sales.csv"
  | pivot_longer q1, q2, q3, q4 names_to quarter values_to revenue
```

### complete

Expand missing key combinations and optionally fill values.

```pdl
load "sales.csv"
  | complete region, month fill revenue = 0
```

### save

Write the current table.

```pdl
load "orders.csv"
  | save "orders.parquet"

load "orders.csv"
  | save stdout format "csv"

load "orders.csv"
  | save - format "arrow-stream"
```

Save options:

```pdl
save "out.csv" format "csv" overwrite true header true
```

`save stdout` and `save -` both write to standard output. Use CLI
`--stdout-format` for ordinary stdout output unless the source intentionally
owns the stdout format.

## Complete Grammar Cheatsheet

This is the current surface shape in compact form:

```ebnf
Program          ::= ContextDecl* TopLevelItem*
ContextDecl      ::= ("param" | "state") Ident "=" Literal
TopLevelItem     ::= BindingDecl | OutputDecl | Pipeline
BindingDecl      ::= "let" Ident "=" Pipeline
OutputDecl       ::= "output" Ident "=" Pipeline
Pipeline         ::= PipelineStart PipelineStage*
PipelineStart    ::= LoadStage | Ident
PipelineStage    ::= "|" (TransformStage | SaveStage)

LoadStage        ::= "load" (String | "stdin") FormatClause?
FormatClause     ::= "format" (String | Ident)
SaveStage        ::= "save" (String | "stdout" | "-") FormatClause? SaveOption*
SaveOption       ::= "overwrite" BoolLiteral | "header" BoolLiteral

FilterStage      ::= "filter" ValueExpr
SelectStage      ::= "select" SelectItem ("," SelectItem)*
SelectItem       ::= ColumnRef | ColumnName "=" ColumnRef
DropStage        ::= "drop" ColumnRef ("," ColumnRef)*
RenameStage      ::= "rename" RenameItem ("," RenameItem)*
RenameItem       ::= ColumnName "=" ColumnRef
MutateStage      ::= "mutate" Assignment ("," Assignment)*
Assignment       ::= ColumnName "=" ValueExpr
GroupByStage     ::= "group_by" ColumnRef ("," ColumnRef)*
AggStage         ::= "agg" AggItem ("," AggItem)*
AggItem          ::= ColumnName "=" AggCall
SortStage        ::= "sort" SortItem ("," SortItem)*
SortItem         ::= ColumnRef ("asc" | "desc")? ("nulls_first" | "nulls_last")?
LimitStage       ::= "limit" IntLiteral
JoinStage        ::= "join" Ident "on" JoinKey ("," JoinKey)* ("kind" JoinKindName)?
JoinKey          ::= ColumnRef | "(" ColumnRef "," ColumnRef ")"
JoinKindName     ::= "inner" | "left" | "right" | "full" | "semi" | "anti"
UnionStage       ::= "union" Ident UnionOption*
UnionOption      ::= "by_name" BoolLiteral | "distinct" BoolLiteral
DistinctStage    ::= "distinct" ColumnRefList?
PivotLongerStage ::= "pivot_longer" ColumnRefList "names_to" ColumnName "values_to" ColumnName
CompleteStage    ::= "complete" ColumnRefList ("fill" CompleteFillItem ("," CompleteFillItem)*)?
CompleteFillItem ::= ColumnName "=" ValueExpr
ColumnRefList    ::= ColumnRef ("," ColumnRef)*
```

Do not generate syntax outside this shape.

## Expressions

Use column names directly in expressions.

```pdl
amount > 100
status == "completed"
region == "West" or region == "North"
not cancelled
amount + tax - discount
amount / quantity
```

Literal values:

```pdl
"text"
123
123.45
true
false
null
```

Operators by precedence:

```text
not, !, unary -
*, /, %
+, -
<, <=, >, >=
==, !=
and, &&
or, ||
```

Comparison chaining is not supported. Write `a < b and b < c`, not
`a < b < c`.

Column references are bare identifiers or backtick-delimited names:

```pdl
select order_id, `Gross Amount`, `sort`
```

String literals are not column references. Use backticks for unusual column
names, not double quotes.

All shipped scalar functions:

```pdl
is_null(value)
not_null(value)
coalesce(value, ...)
concat(value, ...)
lower(value)
upper(value)
trim(value)
contains(text, "needle")
starts_with(text, "prefix")
replace(text, "old", "new")
to_string(value)
to_number(value)
to_boolean(value)
abs(value)
round(value)
round(value, digits)
if_else(condition, when_true, when_false)
col("column_name")
```

`round(value, digits)` requires integer literal `digits` from 0 through 12.
String concatenation should use `concat(...)`; do not rely on `+` for text.

Temporal scalar functions return existing value classes:

```pdl
date(value)
datetime(value)
year(value)
month(value)
day(value)
date_floor(value, "month")
date_format(value, "%Y-%m")
```

`date_floor` units are `"day"`, `"week"`, `"month"`, and `"year"`. The week
floor is ISO Monday. `date_format` supports the deterministic token subset
`%Y`, `%m`, `%d`, `%H`, `%M`, `%S`, `%G`, `%V`, `%u`, `%j`, `%z`, `%:z`, and
`%%`.

Date and datetime parsing is deterministic. Unparseable temporal input returns
null rather than executing host-specific date logic.

All shipped aggregate functions:

```pdl
count()
count(column)
sum(value)
mean(value)
min(value)
max(value)
count_distinct(value)
```

`count()` counts rows. `count(column)` counts non-null values. Aggregates are
valid in `agg` and aggregate window expressions.

## Window Expressions

Window functions are used inside `mutate` with `over (...)`.

```pdl
load "sales.csv"
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

Common window functions include `row_number()`, `rank()`, `dense_rank()`,
`percent_rank()`, `cume_dist()`, `lag(value, offset, default)`,
`lead(value, offset, default)`, `first_value(value)`, `last_value(value)`, and
aggregate functions such as `sum(value)` and `mean(value)`.

Named frames:

```pdl
sum(amount) over (
  partition_by customer_id
  order_by order_date
  frame running
)

sum(amount) over (
  partition_by customer_id
  order_by order_date
  frame trailing 3
)
```

Frame names include `whole_partition`, `running`, `remaining`, `trailing N`,
`leading N`, and `centered N`.

All shipped window functions:

```pdl
row_number()
rank()
dense_rank()
percent_rank()
cume_dist()
lag(value)
lag(value, offset)
lag(value, offset, default)
lead(value)
lead(value, offset)
lead(value, offset, default)
first_value(value)
last_value(value)
count()
count(value)
sum(value)
mean(value)
min(value)
max(value)
```

Window expressions are only valid in `mutate` assignments and must use
`over (...)`.

## Context Declarations

Reactive host integrations may declare parameters and state before pipelines:

```pdl
param metric_column = "amount"
param cutoff = 100
state selected_region = null

load "sales.csv"
  | filter col($metric_column) > $cutoff
  | filter is_null(@selected_region) or region == @selected_region
```

`$name` reads a `param`. `@name` reads a `state`. Defaults must be string,
number, boolean, or null literals. Use `col($name)` when a string context value
should be interpreted as a column name.

## Formats

Built-in format names:

```text
csv
jsonl
parquet
arrow-file
arrow-stream
```

Use `arrow-stream` for Unix pipes. Use `format "..."` in source only when the
program should own the format choice; otherwise prefer CLI flags.

```pdl
load stdin format "csv"
  | save stdout format "arrow-stream"
```

## Reserved Words

Do not use these as binding or output names. Backtick a column name when it
collides with one of them.

```text
load save filter select drop rename mutate group_by agg sort limit join union
distinct pivot_longer complete let output param state on kind by_name names_to
values_to fill format stdin stdout true false null and or not asc desc inner
left right full semi anti nulls_first nulls_last over partition_by order_by
frame whole_partition running remaining trailing leading centered rows between
unbounded_preceding current_row unbounded_following preceding following
```

## CLI Commands

```bash
pdl check file.pdl
pdl fmt file.pdl
pdl fmt file.pdl --check
pdl run file.pdl --stdout-format csv
pdl run file.pdl --stdout-format arrow-stream > out.arrow
pdl schema file.pdl
pdl schema file.pdl --binding binding_name --json
pdl plan file.pdl --json
pdl manifest file.pdl
pdl ast file.pdl
pdl ir file.pdl
pdl init --codex
pdl init --claude
pdl init --agy
pdl lsp
pdl version
```

Backend selection:

```bash
pdl run file.pdl --engine auto
pdl run file.pdl --engine row
pdl run file.pdl --engine row-strict
pdl run file.pdl --engine native
pdl run file.pdl --engine native-strict
```

Use `auto` unless you are testing engine behavior.

## Common Agent Pitfalls

- Do not write SQL syntax in `.pdl` files.
- Do not use `|>`; PDL pipeline stages use `|`.
- Do not put commas after single-item stage lists unless the existing style
  does so.
- Do not invent `where`, `from`, `select *`, `group by`, or SQL joins.
- Do not use Python-like `def`, `for`, `while`, list comprehensions, lambdas, or
  imports.
- Do not assume PDL mutates source files in place.
- Do not use `save` when the desired result is stdout.
- Do not write operational text to stdout in examples that are meant to stream
  data.
- For multi-output work, use named outputs rather than trying to print several
  tables to one stdout stream.
- If two outputs need the same intermediate result, use a `let` binding for
  that intermediate table. Do not reference an `output` name as if it were a
  reusable binding.
- For multi-output files with `save` stages, run `pdl run file.pdl` without
  `--stdout-format`; otherwise PDL will reject the program because multiple
  outputs cannot share one stdout stream.
- Prefer explicit `select` at the end of examples so output order is stable.
- Prefer `sort` before `limit` when top-N order matters.
- Use `by_name true` for unions unless positional column order is intentional.
- If diagnostics mention unknown columns, inspect the schema with
  `pdl schema file.pdl`.
- If formatting fails because comments are present, do not force a rewrite.

## Project Agent Setup

If this file was generated by `pdl init`, keep it at the project root and have
agent instruction files reference it. Do not paste this whole reference into
every agent file.

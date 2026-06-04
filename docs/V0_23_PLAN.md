# PDL v0.23 Plan

Status: Shipped
Target version: 0.23.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_22_PLAN.md`](V0_22_PLAN.md)
Roadmap theme: Story-table preparation for the Datafarm Studio bike-share case study.

## Purpose

PDL v0.23 adds the small table-shaping surface needed for the Datafarm Studio
bike-share story to be written as one compact, inspectable preparation program.

The release is intentionally narrow: it supports decimal rounding, distinct
counts, wide-to-long reshaping, missing key completion, and named materialized
outputs. These features remove the current workaround tables and repeated
per-chart programs without turning PDL into an imperative workflow language.

The motivating acceptance target is the bike-share story-table shape: one PDL
document prepares the five chart-ready CSV tables used by the Studio app and by
the Algraf story charts.

## Must

- Add decimal-place rounding and distinct counts.

  Status: Shipped in 0.23.0. `round(value)` MUST preserve the current nearest-integer
  behavior, while `round(value, digits)` MUST round numeric values to a literal
  integer digit count from `0` through `12` and normalize negative zero to `0`
  in deterministic output. `count_distinct(expr)` MUST count unique non-null
  values within an aggregate group; `n_distinct(expr)` MAY be accepted as a
  compatibility alias only if it is implemented consistently across parser,
  semantics, runtime, editor services, docs, and tests.

  Acceptance criteria:

  - Parser, semantic analyzer, runtime, CLI, WASM, formatter, completions, and
    hover describe the supported arities and diagnostics.
  - Runtime tests cover decimal rounding, null propagation, invalid digit
    counts, negative-zero normalization, and grouped distinct counts.
  - `docs/PDL_SPEC.md` records the exact v0.23 behavior and diagnostic rules.

- Add a `pivot_longer` stage for story metric reshaping.

  Status: Shipped in 0.23.0. Syntax is:

  ```pdl
  pivot_longer "Share of rides", "Share of revenue" names_to "metric" values_to "share"
  ```

  The stage MUST emit one output row for each selected source column per input
  row, copy non-selected columns through, remove selected source columns, and
  preserve row order as input row order followed by selected-column order.

  Acceptance criteria:

  - Parser, CST, formatter, AST, semantic IR, analyzer, runtime, editor
    completions, hover, and manifest/render JSON understand the stage.
  - Diagnostics cover missing source columns, duplicate selected columns,
    empty selected column lists, and `names_to`/`values_to` collisions.
  - Runtime tests prove stable schema and row ordering.

- Add a `complete` stage for explicit missing category rows.

  Status: Shipped in 0.23.0. Syntax is:

  ```pdl
  complete "trip_date", "rider_type" fill "trips" = 0, "revenue" = 0
  ```

  The stage MUST build the Cartesian product of observed key-column values in
  first-appearance order and insert one row for each missing key tuple. Existing
  rows are preserved. Filled columns receive their fill expressions; unfilled
  non-key columns are null in inserted rows.

  Acceptance criteria:

  - Parser, formatter, semantic IR, analyzer, runtime, editor completions, and
    hover support the stage and fill assignment syntax.
  - Diagnostics cover no key columns, unknown key columns, duplicate keys,
    duplicate fill targets, fill assignment to a key column, and ambiguous
    duplicate input rows for the same key tuple.
  - Runtime tests cover deterministic ordering and the bike-share daily rider
    table with explicit zero visitor rows on rainy days.

- Add named `output` declarations.

  Status: Shipped in 0.23.0. Syntax mirrors top-level `let` declarations:

  ```pdl
  output daily_rider_trips =
    cleaned
    | group_by "trip_date", "rider_type"
    | agg count() as "trips"
    | save "daily_rider_trips.csv"
  ```

  A document with one or more `output` declarations MUST evaluate outputs in
  source order. Existing single-main-pipeline documents keep current behavior.
  Output pipelines may reference `let` bindings using the existing binding
  rules. `save` remains the output boundary stage; `output` names a materialized
  result and does not replace `save`.

  Acceptance criteria:

  - Parser, AST/CST, formatter, analyzer, semantic IR, driver planning, runtime,
    manifest output, CLI render JSON, LSP/editor services, and WASM ABI expose
    output declarations.
  - CLI behavior is deterministic for documents with multiple saved outputs.
    If multiple unsaved outputs would need stdout, the CLI MUST emit a clear
    diagnostic instead of mixing data streams.
  - WASM run JSON returns named output tables so browser hosts can select the
    prepared table for each chart.
  - Diagnostics cover duplicate output names, output/binding conflicts, unknown
    output selection if a selector is added, and ambiguous documents that mix a
    main pipeline with output declarations.

- Land the bike-share story program as a regression fixture.

  Status: Shipped in 0.23.0. The in-repository regression fixture prepares
  these outputs from an embedded bike-share CSV shape without depending on story
  files outside the `pdl` directory:

  - `daily_rider_trips.csv`
  - `valid_trips.csv`
  - `revenue_inversion.csv`
  - `weather_split.csv`
  - `dock_priority.csv`

  Acceptance criteria:

  - The generated CSVs match the asserted bike-share expected outputs, modulo
    deliberate decimal-format changes documented in this plan and the spec.
  - The fixture runs through the native parser, driver, analyzer, planner, and
    runtime path; WASM has a separate named-output ABI regression.
  - The Studio app can consume the WASM named-output payload without per-step
    duplicate PDL programs.

## Should

- Keep current shipped syntax and behavior compatible while adding the story
  features.

  Status: Shipped in 0.23.0. Existing PDL examples, CLI behavior, LSP diagnostics, and
  WASM browser run behavior should continue to pass unchanged unless this plan
  explicitly changes them.

- Update editor affordances alongside language support.

  Status: Shipped in 0.23.0. Monaco/LSP completions, hover text, semantic tokens where
  applicable, document symbols, formatting, and diagnostics should be useful for
  the new stages and declarations before the Studio story depends on them.

- Keep v0.23 focused on table preparation, not workflow orchestration.

  Status: Shipped in 0.23.0. Named outputs are materialized table results, not tasks,
  loops, conditional execution, or side-effect scheduling.

## Non-Goals

- Do not add a `write` alias for `save`.
- Do not add aggregate assignment syntax such as `trips = count()`.
- Do not add `join ... on "a" == "b"` syntax.
- Do not add column-total aggregate calls in ordinary `mutate` expressions
  unless they are already valid window expressions.
- Do not add a general table-workflow engine, loops, or arbitrary user-defined
  functions.

## Validation

Required checks before this plan can be marked shipped:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Additional release validation:

- Native CLI regression for the bike-share story program.
- WASM ABI regression proving named outputs are available to the Studio host.
- Documentation/spec review confirming `docs/PDL_SPEC.md`, this plan, examples,
  and version stamps all agree on v0.23 behavior.

## Deferred

- Comment-preserving formatter rewrites.
- Configurable formatter width or style options.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work.

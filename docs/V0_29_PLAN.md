# PDL v0.29 Plan

Status: Implemented
Target version: 0.29.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_28_PLAN.md`](V0_28_PLAN.md)
Downstream coordination: `../algraf/docs/V0_64_PLAN.md`,
`../studio/docs/V0_4_PLAN.md`

## Purpose

PDL v0.29 introduces the language/runtime foundation for reactive Datafarm
workflows. The release goal is to let a host application compile a PDL program
once, then re-evaluate it against a typed context map containing host-driven
parameters and view-driven state without text substitution or reparsing.

The intended split is:

- PDL owns parameter/state declaration, tokenization, analysis, context value
  coercion, and runtime evaluation.
- Algraf remains a read-only graphics presenter that emits inert interaction
  metadata.
- Studio owns UI controls, event routing, and deciding which Algraf emission
  updates which PDL state binding.

This plan is not normative until the same behavior is promoted into
[`PDL_SPEC.md`](PDL_SPEC.md) with concrete grammar, diagnostics, and runtime ABI
language. Implementation must keep examples runnable against the accepted PDL
syntax in the working tree.

## Must

- Add first-class reactive context declarations.

  Status: Implemented. The parser, formatter, AST/CST views, semantic analyzer, IR,
  editor services, LSP, CLI checks, and WASM runtime MUST support top-level
  declaration forms for host-supplied parameters and host-observed state:

  ```pdl
  param time_cutoff = 15
  param active_fleet = "all"
  state selected_zone = "Downtown"
  ```

  `param` declarations define externally controlled inputs such as sliders,
  select boxes, or query settings. `state` declarations define named values that
  a host may update from visualization or workspace interactions. Both forms
  declare defaults so a script remains analyzable and runnable when no external
  context value is supplied.

- Tokenize `$` and `@` references as context bindings.

  Status: Implemented. `$name` MUST resolve to a declared parameter and `@name` MUST
  resolve to a declared state value. Bare identifiers such as `status`,
  `amount`, and `region` MUST continue to tokenize and analyze as column
  references in column/read positions. Backtick-escaped names MUST continue to
  represent column references for names with spaces or punctuation. The sigils
  are the lexical boundary between table columns and context values.

- Validate declarations and references with stable diagnostics.

  Status: Implemented. The analyzer MUST reject duplicate context declarations,
  unknown `$`/`@` references, invalid default values, invalid external context
  value types, and attempts to use context references where the target grammar
  cannot accept them. New diagnostic codes MUST be reserved in
  [`PDL_SPEC.md`](PDL_SPEC.md) before code emits them.

- Evaluate from a host context map without reparsing.

  Status: Implemented. Native and WASM runtime APIs MUST expose an evaluation path
  that accepts source, files, and an optional context map keyed by declaration
  name. The compiler/analyzer output should be reusable so changing
  `time_cutoff`, `active_fleet`, or `selected_zone` updates execution inputs
  without source text replacement. If no host value is provided, the declaration
  default is used.

- Support context values in expressions.

  Status: Implemented. `$name` and `@name` MUST be valid value expressions anywhere
  an expression can consume the declared value type. Example:

  ```pdl
  load "trips.csv"
    | filter duration_min >= $time_cutoff
    | filter zone == @selected_zone
  ```

  Context references MUST remain deterministic. They are explicit runtime
  inputs, not reads from environment variables, process state, wall-clock time,
  random state, or the host filesystem.

- Define column-position coercion for context values.

  Status: Implemented. Stages that require column names, such as `sort`,
  `group_by`, `join ... on (...)`, `distinct`, `pivot_longer`, `complete`, and
  window `partition_by`/`order_by`, MUST define whether and how a context value
  can resolve to a column reference. The intended behavior is that a string
  parameter in a column-name position is coerced to the active column reference:

  ```pdl
  param sort_metric = "total_revenue"

  load "rankings.csv"
    | sort $sort_metric desc
  ```

  Coercion failures, unknown resolved columns, and non-string context values in
  column positions MUST produce targeted diagnostics.

- Add explicit dynamic-column indirection for expression contexts.

  Status: Implemented. Boolean and arithmetic expressions MUST preserve the
  difference between comparing a column to a context value and resolving a
  context value as a column name. A helper such as `col($metric_column)` MAY be
  promoted for this narrow indirection use:

  ```pdl
  param active_fleet = "all"
  param metric_column = "revenue"

  load "summary.csv"
    | filter fleet == $active_fleet
    | filter col($metric_column) > 500
  ```

  Because earlier PDL releases retired the old `col(...)` authoring surface,
  this item MUST explicitly reconcile the new dynamic-column behavior in the
  spec before implementation.

- Preserve existing tabular language behavior.

  Status: Implemented. Existing accepted syntax, table transformations, file
  formats, CLI stdout/stderr behavior, semantic token categories, editor
  package APIs, and browser demo behavior MUST remain unchanged except where
  the new reactive context surface is explicitly integrated.

## Should

- Keep the spec, plan, examples, package manifests, editor assets, and
  implementation aligned with any promoted scope.

  Status: Implemented.

- Keep host integrations thin.

  Status: Implemented. Browser hosts such as Studio should pass context maps and
  files into the PDL runtime rather than implementing PDL parameter parsing,
  source rewriting, expression evaluation, column coercion, or diagnostics in
  TypeScript.

- Surface context bindings in editor features.

  Status: Implemented. Completion, hover, semantic tokens, rename/reference, symbols,
  formatting, and static grammar assets should understand `param`, `state`,
  `$name`, and `@name` well enough that authors can distinguish columns, table
  bindings, parameters, and states while editing.

- Keep the first runtime ABI small.

  Status: Implemented. The first WASM/browser surface should support a JSON context
  map and generated output files. Incremental graph invalidation, persistent
  compiled handles, and sub-millisecond benchmarking can be deferred unless
  needed to make the basic reactive loop correct.

- Add a runnable cross-repo example.

  Status: Implemented. Add or update at least one example that prepares a selector
  table and a context-filtered receiver table, suitable for Studio to pair with
  Algraf event metadata:

  ```pdl
  param active_fleet = "all"
  state selected_zone = "Downtown"

  let trips =
    load "trips.csv"
    | filter $active_fleet == "all" or fleet == $active_fleet

  output zone_summary =
    trips
    | group_by zone
    | agg total_revenue = sum(revenue)
    | save "zone_summary.csv"

  output active_rankings =
    trips
    | filter zone == @selected_zone
    | group_by station
    | agg total_revenue = sum(revenue)
    | sort total_revenue desc
    | save "active_rankings.csv"
  ```

## Validation

Required checks before this plan can be marked landed:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Focused validation should cover:

- Parser and formatter support for `param`, `state`, `$name`, and `@name`.
- Analyzer diagnostics for duplicate declarations, unknown references, invalid
  context types, invalid defaults, and unknown dynamically resolved columns.
- Runtime evaluation with defaults and with host-supplied context overrides.
- Repeated evaluation against the same source with different context values,
  proving no source text rewriting is required.
- WASM JSON ABI tests for passing context maps and returning generated files.
- Editor-service, LSP, TextMate, and Monaco tests or checks for the new token
  categories and completions.
- Existing examples and runtime tests to confirm non-reactive PDL behavior is
  unchanged.

## Deferred

- Configurable CSV dialect options.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output.
- Output selectors and full multi-output browser controls.
- Persisted compiled-program handles or explicit incremental invalidation APIs.
- Complex parameter schemas such as min/max/step/options metadata unless they
  are needed for the initial Studio integration.

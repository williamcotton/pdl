# PDL v0.26 Plan

Status: Complete
Target version: 0.26.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_25_PLAN.md`](V0_25_PLAN.md)

## Purpose

PDL v0.26 is a breaking pre-1.0 syntax cleanup release. It removes
context-sensitive quoted column references, reserves double quotes for
string/path literals, introduces backtick-escaped column references, and makes
all column-producing stages use left-hand assignment.

The release goal is a smaller, more deterministic language surface:

- bare identifiers name columns in expression and column positions;
- backticks escape columns with spaces, punctuation, or keyword collisions;
- double quotes always mean text or paths;
- `mutate`, `agg`, `select`, and `rename` all put the resulting column name on
  the left side of `=`;
- the `as`, `col(...)`, and `lit(...)` authoring surfaces are removed from the
  preferred v0.26 grammar.

Studio migration is tracked separately in
`../studio/docs/V0_2_PLAN.md`. PDL v0.26 owns the language, runtime,
formatter, editor-service, WASM, CLI, examples, PDL documentation, release
documentation, editor assets, and the PDL browser demo changes.

## Must

- Replace quoted column references with bare and backtick column syntax.

  Status: Complete. The lexer/parser MUST recognize ordinary column references
  as identifiers in expression and column positions, and escaped column
  references as backtick-delimited names. Double-quoted tokens MUST remain valid
  only as string/path literals.

- Remove context-sensitive quoted-column disambiguation.

  Status: Complete. The analyzer MUST stop inferring whether a quoted token is a
  column or string from operand position. Likely v0.25 quoted-column input
  SHOULD produce targeted migration diagnostics pointing to bare identifiers or
  backticks.

- Replace aggregate aliases with assignment-form aggregate items.

  Status: Complete. `agg revenue = sum(amount), orders = count()` MUST replace
  `agg sum("amount") as "revenue", count() as "orders"`.

- Remove `as` from column-producing grammar.

  Status: Complete. `select` inline renames, `rename`, and `agg` MUST use
  left-hand assignment:

  ```pdl
  select order_id = `Order ID`, amount
  rename revenue = gross_amount
  agg revenue = sum(amount), orders = count()
  ```

- Update the formatter, examples, public docs, and demo sources to the v0.26
  syntax.

  Status: Complete. The formatter SHOULD emit bare names for simple columns and
  backticks for non-simple or reserved column names. Repository examples and
  documentation snippets MUST be migrated before the release is marked shipped.
  This includes checked-in `.pdl` examples, README snippets, generated or
  bundled documentation content, and browser demo source strings such as
  `demo/src/pages/HomePage.tsx`, `demo/src/pages/DemoPage.tsx`, and
  `demo/src/pages/docs/content.tsx`.

- Update static editor and highlighting assets.

  Status: Complete. VS Code TextMate grammar assets and browser demo grammar
  consumption MUST recognize backtick-escaped column references, keep
  double-quoted tokens scoped as strings/paths, stop highlighting `as` as
  active v0.26 alias syntax, and scope scalar, aggregate, and window function
  calls consistently for both VS Code and Monaco static highlighting.

- Keep editor, LSP, WASM, CLI, and semantic behavior aligned.

  Status: Complete. Semantic tokens, hover, completion, rename/reference,
  document symbols, diagnostics, browser editor services, CLI JSON
  introspection, and WASM execution MUST use the same grammar and semantic
  rules as native parsing.

- Preserve window frames and offset window functions under the new syntax.

  Status: Complete. Existing window syntax, including
  `rows between unbounded_preceding and current_row`, `lag(...)`, and
  `lead(...)`, MUST remain valid with bare/backtick column references and
  assignment-form `mutate` targets.

## Should

- Provide practical migration diagnostics for old syntax.

  Status: Complete. Quoted tokens in column positions should suggest a bare name
  when the old column is a simple identifier and a backtick escape otherwise.
  Old `expr as name` aggregate, select, or rename items should suggest
  `name = expr`.

- Preserve non-language IO and ABI behavior unless required by the syntax
  change.

  Status: Complete. CLI commands, file formats, driver behavior, output
  materialization, manifest shape, and stable WASM run fields should remain
  compatible except where source text or editor-service syntax payloads
  necessarily change.

- Keep cross-repo rollout explicit.

  Status: Tracked separately. Studio's source migration, bundled grammar copy, and WASM
  consumption assumptions belong in `../studio/docs/V0_2_PLAN.md`.

- Keep the PDL browser demo current with the shipped language.

  Status: Complete. Demo route presets, live docs examples, homepage starter
  code, tutorial copy, Monaco highlighting, and any embedded output examples
  should teach and execute v0.26 syntax. Monaco theme rules style shared
  TextMate function scopes, including aggregate-style window calls such as
  `sum(amount) over (...)` and ranking functions such as `row_number()` and
  `dense_rank()`.

## Validation

Required checks before this plan can be marked shipped:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Status: Passed for the v0.26 implementation.

Additional release validation:

- Parser tests cover bare columns, backtick columns, function-call lookahead,
  keyword collisions, and old quoted-column diagnostics.
- Semantic tests cover `filter`, `select`, `rename`, `mutate`, `group_by`,
  `agg`, `sort`, `join`, `distinct`, `pivot_longer`, `complete`, and window
  expressions using v0.26 syntax.
- Window tests cover explicit `rows between unbounded_preceding and current_row`
  frames, `lag(...)`, `lead(...)`, partitioning, ordering, and frame/offset
  diagnostics.
- Formatter snapshots cover assignment-form column-producing stages and
  bare/backtick column rendering.
- LSP/editor-service/WASM tests cover completion, hover, semantic tokens,
  diagnostics, rename/reference, formatting, and browser execution.
- CLI example tests run migrated examples and preserve deterministic outputs.
- Browser demo validation runs from `demo/` after demo source or runtime assets
  change:

  ```bash
  npm run check
  npm run build
  ```

  Status: Passed for the v0.26 browser demo.

  Manual demo review should confirm the homepage starter, docs live examples,
  demo presets, editor diagnostics, function highlighting, and WASM execution
  all use the v0.26 syntax.

## Deferred

- Studio story migration and browser validation, tracked in
  `../studio/docs/V0_2_PLAN.md`.
- Binary virtual file payloads for Arrow IPC, Arrow file, or Parquet saves.
- Output selectors and richer browser output controls.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Additional window frame modes such as range frames and exclude clauses.

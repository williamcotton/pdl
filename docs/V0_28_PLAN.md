# PDL v0.28 Plan

Status: Implemented
Target version: 0.28.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_27_PLAN.md`](V0_27_PLAN.md)

## Purpose

PDL v0.28 improves editor readability by promoting column and table-binding
highlighting from generic variable coloring to explicit semantic tokens. In
programs that define a table binding and then transform columns from that table,
for example `let cleaned = ...` followed by `cleaned | group_by region_channel`,
the editor should clearly distinguish the table binding `cleaned` from column
references such as `region_channel`, `net_amount`, and `revenue`.

This release is about editor intelligence and presentation only. It does not add
new PDL source syntax, parser grammar, runtime behavior, dataframe semantics, or
execution planning semantics. Studio and other browser hosts should receive the
behavior through the shared PDL editor/WASM packages rather than implementing
PDL-specific token classification in TypeScript.

## Must

- Add binding and column semantic token categories.

  Status: Implemented. `pdl-editor-services` MUST expose semantic token kinds for
  table binding declarations, table binding references, column definitions, and
  column references. These categories MUST no longer collapse into the existing
  generic variable token kind.

- Classify semantic tokens from parsed PDL structure.

  Status: Implemented. `semantic_tokens(source)` MUST use parsed program structure
  for binding and column names instead of relying only on scanner-level
  identifier classification. `let cleaned = ...` MUST classify `cleaned` as a
  binding declaration. Pipeline starts such as `cleaned | ...` MUST classify
  `cleaned` as a binding reference.

- Highlight column references in read positions.

  Status: Implemented. Existing column reads MUST be classified as column
  references in filter expressions, mutate expressions, aggregate arguments,
  group keys, sort keys, distinct keys, join keys, pivot source columns,
  complete keys, complete fill expressions, and window partition/order
  clauses.

- Highlight produced column names separately.

  Status: Implemented. Column names introduced or rewritten by output positions
  MUST be classified as column definitions. This includes mutate assignment
  targets, aggregate aliases, select aliases, rename destination names,
  pivot-longer `names_to` and `values_to` names, and complete fill targets.

- Propagate the token contract through editor hosts.

  Status: Implemented. The LSP semantic-token legend, WASM/editor-service JSON ABI,
  TypeScript runtime/editor package types, Monaco semantic-token legend, token
  encoder, and default PDL theme MUST recognize the new binding and column token
  categories.

- Preserve existing non-name token behavior.

  Status: Implemented. Comments, strings, numbers, operators, keywords, and current
  function highlighting MUST keep their existing behavior unless a change is
  required to integrate the new semantic-token legend.

- Avoid host-side PDL language logic.

  Status: Implemented. Browser hosts such as the PDL demo and Studio MUST NOT
  reimplement PDL parsing or semantic highlighting in TypeScript. They should
  consume the shared PDL editor/WASM surfaces updated by this release.

- Keep source-language and runtime behavior unchanged.

  Status: Implemented. The release MUST NOT change accepted PDL syntax, diagnostics,
  execution planning, runtime outputs, file formats, or dataframe behavior.

## Should

- Keep PDL spec, plans, examples, editor assets, packages, and implementation
  aligned with any promoted scope.

  Status: Implemented.

- Make related column styles visually coherent.

  Status: Implemented. Column definitions and column references should read as part
  of the same visual family while still being distinguishable. Binding styles
  should be distinct enough that table bindings such as `cleaned` do not look
  like columns such as `region_channel`.

- Preserve static highlighting as a useful fallback.

  Status: Implemented. TextMate grammar scopes and static theme rules should remain
  useful for environments that do not request semantic tokens. Static
  highlighting does not need to fully reproduce parser-backed binding and column
  classification.

- Keep deeper token categories out of this release unless required.

  Status: Implemented. Scalar, aggregate, and window function subcategories may be
  left as future work if function highlighting already remains at least as good
  as v0.27.

- Document downstream integration expectations.

  Status: Implemented. Package or release documentation should note that downstream
  apps receive the new highlighting through updated PDL editor services,
  `pdl-wasm`, and `pdl-editor` surfaces.

## Validation

- `pdl-editor-services` tests cover semantic token classification for a program
  shaped like:

  ```pdl
  let cleaned =
    load "orders_raw.csv"
    | filter lower(trim(status)) == "completed"
    | mutate
        net_amount = gross_amount - coalesce(discount, 0),
        region_channel = concat(upper(trim(region)), ":", lower(trim(channel)))
    | distinct order_id

  cleaned
    | group_by region_channel
    | agg orders = count(), revenue = sum(net_amount)
    | sort revenue desc
  ```

- Tests assert `cleaned` in the `let` declaration is a binding declaration and
  the later pipeline-start `cleaned` is a binding reference.
- Tests assert `net_amount`, `region_channel`, `orders`, and `revenue` in output
  positions are column definitions.
- Tests assert read-position columns such as `status`, `gross_amount`,
  `discount`, `region`, `channel`, `order_id`, `region_channel`, `net_amount`,
  and `revenue` are column references.
- LSP and WASM/TypeScript tests or type checks verify that the expanded semantic
  token legend and serialized token kind names remain aligned.
- Monaco package checks verify that the expanded token legend and default theme
  compile.
- Run the relevant checks from the PDL repository root:

  ```bash
  cargo fmt --all --check
  cargo test -p pdl-editor-services
  cargo test -p pdl-lsp
  cargo test -p pdl-wasm
  ```

  If package TypeScript surfaces change, also run the relevant package
  type/build checks under `packages/wasm/` and `editors/monaco/`.

## Deferred

- Schema-aware styling for unknown, ambiguous, or unresolved columns.
- Separate semantic token categories for scalar, aggregate, and window
  functions.
- Token modifiers for definitions, references, generated columns, deprecated
  syntax, or diagnostics-overlapping symbols.
- Incremental or range semantic-token providers beyond the existing full-document
  semantic-token flow.

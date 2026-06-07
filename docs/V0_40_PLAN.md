# PDL v0.40 Plan

Status: Shipped in 0.40.0
Target version: 0.40.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_39_PLAN.md`](V0_39_PLAN.md)
Successor plan: [`V0_41_PLAN.md`](V0_41_PLAN.md)

## Purpose

PDL v0.40 should take the v0.39 window and composite-join release and close the
remaining behavior-sensitive native gaps only where the language semantics are
explicit. The row runtime remains the semantic reference, and browser/WASM
remains native-free, small, and browser-based.

## Native Execution Gap Assessment

To close the remaining native execution gaps and move these operations onto the
fast path, code contributions must expand the query compiler translation layer
between PDL's Intermediate Representation (IR) and native Polars expressions.

The backend already isolates Polars logic behind the strict `pdl-data` facade,
which makes these performance enhancements achievable without leaking Polars
semantics into the language surface.

All v0.40 native work must preserve the existing crate boundaries. Polars types,
APIs, and execution details remain private to `pdl-data`; `pdl-semantics`
continues to own language-level typing and IR production; `pdl-exec` continues
to own engine eligibility, plan segmentation, and execution orchestration through
facade types rather than direct backend APIs. The browser/WASM runtime must not
pull in Arrow, Parquet, or Polars dependencies.

### 1. Window Multi-Key Ordering and Multi-Key Window Functions

Barrier: `crates/pdl-exec/src/planning.rs` currently rejects native execution
when `native_window_unsupported_reason` sees more than one ordering key. In
`crates/pdl-data/src/engine.rs`, `native_window_order` also errors on multi-key
slices because Polars window functions historically applied one `SortOptions`
configuration per window block.

Contribution path: the optimizer can change the execution strategy instead of
depending on inline sorting inside each window expression. The query compiler can
inject a global `Sort` stage into the native `LazyFrame` graph immediately before
the window calculations. Once each partition is physically pre-sorted by
multiple keys with Polars' native multi-key sorting, window operations can run
over the ordered sequence without the current engine limitation.

### 2. Window Offset Functions With Non-Null Defaults

Barrier: `crates/pdl-exec/src/planning.rs` currently returns a fallback reason
when `lag` or `lead` receives a default expression, such as `lag(amount, 1, 0)`.
The row runtime can lazily mix dynamic cell values, while native Polars requires
homogeneous column dtypes.

Contribution path: `crates/pdl-data/src/engine.rs` already has the lowering
shape for custom defaults through a conditional `native::when(out_of_bounds)`
branch. To lift the planner guard safely, `pdl-semantics` must statically prove
that the default expression type is compatible with the shifted column dtype.
Once that proof exists, `native_offset_default_reason` can be relaxed or removed
for proven-compatible defaults.

### 3. Expanded String, Numeric, and Cast-Style Scalar Functions

Barrier: native scalar lowering currently covers only a small set of functions,
including `concat`, `lower`, `upper`, `trim`, `abs`, `round`, `to_number`, and
`if_else`. Other scalar functions trigger a `ScalarFunction` fallback.

Contribution path: this is mostly translation coverage inside
`crates/pdl-data/src/engine.rs`. Add the relevant `DataScalarFunction` variants
and lower them in `native_expr` when Polars provides an equivalent expression.
String functions such as `contains`, `starts_with`, and `replace` can map to
Polars string expressions such as `.str().contains()` and
`.str().starts_with()`. Cast-style functions such as `to_string` and
`to_boolean` can map to `.cast(native::DataType::String)` and
`.cast(native::DataType::Boolean)` once type and null semantics are specified.

### 4. Dynamic Column Indirection Through `col(...)`

Barrier: Polars needs an ahead-of-time execution tree with a known schema. A
row-dependent dynamic column lookup, where the selected column name changes per
row, cannot be vectorized and correctly remains a `DynamicColumn` fallback in
`crates/pdl-exec/src/planning.rs`.

Contribution path: many practical `col()` uses are compile-time constants or
environment parameters, such as `col($target_metric)`. Expanding parameter
pre-processing in `pdl-semantics` to resolve top-level environment variables
before execution planning would turn those inputs into ordinary string literals,
making them eligible for native Polars acceleration while preserving row-runtime
behavior for truly row-dependent lookups.

### Implementation Roadmap

1. Update `docs/PDL_NATIVE_COVERAGE.csv` to mark each targeted item as
   `native parity` or `native partial` so documentation tests stay aligned.
2. Add the needed translation match arms in `native_expr` or
   `native_window_expr` within `crates/pdl-data/src/engine.rs`, keeping
   Polars-specific APIs behind the `pdl-data` facade.
3. Remove or narrow the corresponding safety guards in
   `crates/pdl-exec/src/planning.rs` only after semantics and parity checks make
   the native path sound.
4. Run the snapshot tests and confirm affected execution plans flip
   `selected_engine` from `"row"` to `"native"`.

## Must

- Implement native multi-key window ordering and multi-key window functions.

  Status: Shipped in 0.40.0.

  Native mutate now adds a hidden row index, pre-sorts compatible multi-key
  window groups by partition and composite order keys, evaluates windows over
  that ordered partition, restores original row order, and drops the hidden
  index. The promoted subset covers row number, ranking, distribution, offset,
  value, and aggregate window functions when a mutate stage has one compatible
  multi-key order group. Mixed multi-key order groups remain row-only.

- Implement typed native `lag`/`lead` non-null defaults.

  Status: Shipped in 0.40.0.

  Native `lag` and `lead` now accept omitted, null, and native-compatible
  non-null defaults when the value, offset, default, and window spec all lower
  through the native expression subset. Native typed branch compatibility is
  enforced by the native expression engine; automatic mode falls back to rows if
  a forced native dtype combination cannot execute. Mixed row values remain
  row-runtime behavior.

- Expand native string, numeric, and cast-style scalar functions.

  Status: Shipped in 0.40.0.

  Added `contains`, `starts_with`, literal-pattern `replace`, `to_string`, and
  `to_boolean` to the scalar registry, row runtime, native data facade, planner
  eligibility, editor grammar assets, spec, and parity tests. Dynamic per-row
  `replace` patterns remain row-only because the native backend cannot provide
  the required semantics for that shape.

- Resolve compile-time `col(...)` indirection for native planning.

  Status: Shipped in 0.40.0.

  Native planning and lowering now accept `col(...)` when the argument is a
  string literal or a string context default. Required-source-column
  observability resolves those context defaults so static scan needs stay
  visible. Truly row-dependent dynamic column lookup continues to fall back to
  rows.

- Keep native coverage documentation and plan snapshots aligned.

  Status: Shipped in 0.40.0.

  Update `docs/PDL_NATIVE_COVERAGE.csv`, add the relevant `native_expr` or
  `native_window_expr` translation match arms, narrow or remove matching planner
  guards only after semantics prove safety, and run snapshot tests that confirm
  affected plans flip `selected_engine` from `"row"` to `"native"`.

- Preserve strict API and crate boundaries for native execution work.

  Status: Shipped in 0.40.0.

  Keep Polars-specific types, expressions, and execution behavior contained in
  `pdl-data`. Keep type inference, parameter resolution, and IR validation in
  `pdl-semantics`. Keep native eligibility decisions and fallback planning in
  `pdl-exec`, depending only on public facade contracts rather than concrete
  Polars APIs. Do not introduce dependency cycles or expose backend-specific
  behavior through the language, spec, CLI, LSP, WASM, or editor crates.

- Keep the browser/WASM runtime free of Arrow, Parquet, and Polars.

  Status: Shipped in 0.40.0.

  Do not add Arrow, Parquet, or Polars crates or generated bindings to
  `pdl-wasm`, browser packages, the demo runtime, or editor-facing browser
  bundles. Browser execution must stay lightweight and use the existing
  browser-safe runtime path; native dataframe acceleration remains a host/native
  capability behind `pdl-data`, not a WASM dependency.

- Specify union schema extension before implementation.

  Status: Complete decision, deferred beyond 0.40.0.

  Decide whether PDL supports missing-column null padding, explicit type
  widening, or both. Specify column order, mixed values, diagnostics, and
  row/native/WASM parity before changing execution.

  v0.40 keeps union compatible-schema only. A future implementation must first
  specify missing-column null padding, explicit type widening, column order, and
  diagnostics across row, native, and WASM execution.

- Design non-equi joins.

  Status: Complete decision, deferred beyond 0.40.0.

  Define syntax, ordering, null semantics, cardinality, diagnostics, and native
  eligibility. Do not expose incidental backend non-equi behavior.

  v0.40 exposes no non-equi join syntax or backend incidental behavior. Non-equi
  joins remain a future language-design item.

## Should

- Revisit native text writer and JSON Lines input parity.

  Status: Complete decision, no 0.40.0 behavior change.

  Promote only with byte-for-byte row output parity for CSV/JSON Lines writers
  and deterministic row-equivalent JSON Lines schema inference.

- Revisit `pivot_longer` and `complete` native subsets.

  Status: Complete decision, no 0.40.0 behavior change.

  Promote only narrow subsets with deterministic output order, mixed-value
  behavior, fill semantics, diagnostics, and row/native parity tests.

- Design segmented native planning for binding starts, named outputs, and
  multi-output execution.

  Status: Complete design note, deferred beyond 0.40.0.

  Any design must cover observability, diagnostics, cache boundaries, memory
  behavior, stdout purity, and parity tests before implementation.

## Could

- Evaluate configurable CSV dialect support.

  Status: Deferred.

- Evaluate a browser byte IO ABI for binary host-file contents and Arrow IPC
  output.

  Status: Deferred.

- Evaluate object-store and remote path support with a dedicated security and IO
  plan.

  Status: Deferred.

## Demo site and README CLI documentation alignment

The v0.40 release also surfaces the native CLI story on the demo homepage and
the top-level README so visitors who arrived through the browser path can see
what the standalone Rust binary adds. No language, parser, executor, LSP, or
WASM behavior changes are introduced. Spec normative text is not touched; only
the spec `Status:` line tracks the workspace version.

- Append a "Native execution engine" section to `README.md`.

  Status: Shipped in 0.40.0.

  Documents `--engine auto|row|native`, automatic row fallback, `pdl plan`
  introspection, and the Polars 0.53 native coverage already implemented under
  this release. No new language surface; existing flags and behavior only.

- Add a concrete `pdl … | algraf …` cross-tool pipe example to `README.md`.

  Status: Shipped in 0.40.0.

  Extends section 6 ("Arrow streams") with an Algraf-link callout plus a
  command block showing `--stdout-format arrow-stream` handed to
  `algraf render --data - --data-format arrow-stream`. Mirrors the existing
  Algraf-side example.

- Append a CLI section to the demo homepage.

  Status: Shipped in 0.40.0.

  `demo/src/pages/HomePage.tsx` gains a "On the command line" section between
  the existing feature-card grid and the home band. Includes subcommand chips
  (`run`, `check`, `fmt`, `schema`, `plan`, `manifest`, `lsp`), a Polars-engine
  callout, an Arrow-IPC streaming snippet, and the cross-tool pipe with
  Algraf. Existing hero, install strip, and feature cards are left intact.

- Align demo bash snippets to a single light visual style.

  Status: Shipped in 0.40.0.

  `demo/src/styles.css` shifts the existing dark `.install-strip pre` style
  and the new `.cli-snippet` style to the light token set
  (`background: #fbfbf9`, `color: #1d2a30`, `border: 1px solid #e0e6ea`,
  `font-size: 0.86rem`, `line-height: 1.55`) so the install commands and the
  new CLI snippets read as one family. `.cli-snippet code` resets the inline
  pill styling so command text stays clean inside the snippet block.

- Drop the README pointer to `docs/PDL_SPEC.md`.

  Status: Shipped in 0.40.0.

  The README opening stanza no longer asks readers to navigate to the
  normative spec; the spec remains in `docs/` for implementers but isn't a
  README-level call-out. The "Runnable examples live in `examples/`" line is
  kept.

Validation for this scope: build + check the demo (`cd demo && npm install &&
npm run check && npm run build`) and re-render the README in a markdown viewer
to confirm the new sections flow with surrounding tone. No Rust changes; the
existing workspace checks remain authoritative for the rest of v0.40.

# PDL v0.49 Plan

Status: Shipped
Target version: 0.49.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_48_PLAN.md`](V0_48_PLAN.md)
Successor plan: [`V0_50_PLAN.md`](V0_50_PLAN.md)

## Purpose

PDL v0.49 closes the native-coverage expansion arc started in v0.43 —
and it closes it by **promoting every language-level row-only cell to
native parity**, not by relabeling it. After this release, every PDL
program composed of language features (stages, expressions, sources,
sinks, pipeline shapes) plans and runs on the Polars-backed native
engine under `--engine auto`. The row engine remains the parity
reference, the WASM execution path, and the `--engine row` opt-in,
but no language feature demotes to it on native hosts.

Concretely:

1. Every `row-only by design` and `native partial` cell in
   `PDL_NATIVE_COVERAGE.md` / `PDL_NATIVE_COVERAGE.csv` whose area is
   `stage`, `expression`, `source`, `sink`, or `pipeline-shape` flips
   to `native parity`. The `host/PDL-to-Algraf Arrow IPC` handoff row
   also flips to `native parity`. The only cells that remain
   `row-only by design` after v0.49 are the `host` boundaries
   (`WASM`, `LSP/editor`), which are non-execution observability
   boundaries, not language features.
2. The coverage matrix status vocabulary drops `native partial` and
   `planned native`. Every cell becomes `native parity` or `row-only
   by design`. No intermediate status survives.
3. `--engine native-strict` lands. The flag errors on any fallback,
   including side pipelines (binding, named output, non-terminal
   save). After the promotions in this release it should never fire
   for a language-feature reason; it survives as the CI tripwire
   that keeps it that way.
4. The eligibility tests treat the CSV as the compile-time coverage
   oracle for the v0.49 vocabulary and row-only boundary set, while
   the parity harness and `native-strict` example leg prove the
   runnable language-feature corpus selects native.
5. Execution-path `NativeUnsupportedReason` variants tied to
   promoted language features (`TemporalFunction`, `NonEquiJoin`,
   `UnionNullPadding`, `DataDependentColIndirection`,
   `DataDependentReplacePattern`, `UnsupportedNumericCoercion`, and
   `WindowExpression`) retire from the planner's runtime output for
   valid language-feature pipelines, alongside the variants already
   slated for v0.49 cleanup (`StdinBytesBackedScan`,
   `HostBytesBackedScan`). `InputFormat` stops representing JSON
   Lines, but may survive for genuinely unknown formats. Defensive
   and invalid-program variants (`ScalarFunction`,
   `ScalarFunctionArity`, `AggregateFunction`, `AggregateArity`,
   `RowOnlyStage`, `BindingStartNotEligible`,
   `NamedOutputMixedEngines`, `NonTerminalSaveFanout`,
   `DriverFacts`, `NativeSinkWriter`, `NoRunnableMain`) remain only
   where the input is malformed, unsupported by the language, or
   missing driver facts. The documentation-only host boundaries
   (`WasmTargetGraph`, `EditorService`) remain.

The release is gated by v0.43–v0.48. Byte parity with the row engine
is the acceptance bar for every promotion: the row writer is the
spec, and the parity harness must stay green at every commit.

## Implemented Scope

Three rules hold at every commit:

1. Byte parity. Every promoted cell round-trips byte-identically
   against the row engine through the parity harness, including row
   order, null handling, formatting, and text semantics. A promotion
   that cannot reach byte parity does not land; it blocks the
   release rather than silently relaxing the bar.
2. Eligibility-CSV agreement. The matrix CSV is compiled into the
   native-coverage test suite and now enforces the v0.49 vocabulary
   and row-only boundary set: every row is either `native parity` or
   `row-only by design`, and the only row-only rows are
   `host/WASM` and `host/LSP/editor`. The parity harness and
   `native-strict` example leg prove the planner accepts the
   language-feature examples the matrix marks native.
3. WASM stays Polars-free. `pdl-wasm` Cargo manifest, dependency
   tree, and `wasm32-unknown-unknown` target graph are unchanged.
   Every language feature promoted here continues to run in the
   browser on the row engine; native promotion is a host-side
   execution concern only.

The release introduces no new PDL language surface, no new stage
keywords, and no new functions. Promotions change where existing
programs execute, never what they mean.

Implementation note: the shipped implementation uses native orchestration
materialization at operation boundaries where Polars cannot represent
row-runtime values byte-identically. Dynamic `col`, dynamic `replace`,
mixed-class `if_else`, mixed-class `pivot_longer`, class-changing
`complete`, JSON Lines, temporal functions, null-padding union, and
incompatible window groups stay selected as `native` while evaluating the
semantic boundary through the public row table representation inside
`pdl-data`. No new Polars cargo features were required, and rule 3 above
still holds.

## Must

Every Must below is a promotion of a currently row-only (or
partially row-only) language feature to native execution.
Each lands with: lowering in
`crates/pdl-exec/src/runtime/native_lowering.rs`, eligibility
acceptance in `native_planning.rs`, a coverage matrix flip, parity
harness coverage, a `selected_engine` fixture update traveling in
the same commit, and at least one runnable example exercising the
promoted path.

- Promote temporal functions to native.

  Status: Shipped in 0.49.0.

  `date`, `datetime`, `year`, `month`, `day`, `date_floor`, and
  `date_format` are accepted by the native planner and execute with
  row-runtime semantics at the native orchestration boundary. Parity
  bar: parse acceptance and rejection (including null propagation on
  unparseable input), `date_floor` bucket boundaries, and
  `date_format` output strings match the row runtime byte-for-byte
  across the parity corpus, including non-ASCII format strings.
  Retires `TemporalFunction` from runtime planner output and
  deletes the `temporal functions` row-only matrix row. This is the
  highest-traffic promotion in the release; it lands first because
  aggregate arguments, window arguments, `group_by` keys, and
  `complete` fills all inherit its eligibility.

- Promote JSON Lines sources to native.

  Status: Shipped in 0.49.0.

  Path-backed, stdin, and host-byte JSON Lines inputs are accepted by
  the native planner and materialize through the row reader into the
  native orchestration path. Parity bar: schema inference order,
  per-field typing, missing-field null semantics, and text round-trip
  match the row reader. Retires `InputFormat` as a JSON-Lines planner
  outcome (the variant survives only as the defensive boundary for
  genuinely unknown formats) and flips the `JSON Lines` source row,
  plus the JSON Lines carve-outs in the `load`, `stdin`, and
  `byte-backed host files` rows.

- Promote non-equi joins to native.

  Status: Resolved in 0.49.0.

  No non-equi join syntax is shipped in PDL, and v0.49 intentionally
  adds no new language surface. The stale row-only reserve is retired
  from the coverage matrix rather than implemented as syntax; the
  `join` row is `native parity` for all shipped join forms
  (single-key and composite-key equi-joins across `inner`, `left`,
  `right`, `full`, `semi`, and `anti`). `NonEquiJoin` remains only as
  defensive observability vocabulary for future unshipped syntax
  experiments or public JSON compatibility.

- Promote heterogeneous-schema union (null padding) to native.

  Status: Shipped in 0.49.0.

  Schema-mismatched union participants execute through native
  orchestration with row-runtime null-padding and class-alignment
  rules at the union boundary. Parity bar: padded column order, null
  placement, and any class coercion match the row engine. Retires
  `UnionNullPadding` and completes the `union` row.

- Promote data-dependent `col(...)` indirection to native.

  Status: Shipped in 0.49.0.

  When the `col` argument is a computed expression, native
  orchestration evaluates the per-row column-name lookup with the
  row runtime's schema and no-match rules. Parity bar: per-row
  selection, no-match behavior, and class mixing match the row
  engine. Retires `DataDependentColIndirection` and completes the
  `dynamic col` row.

- Promote dynamic `replace` patterns to native.

  Status: Shipped in 0.49.0.

  `replace` with expression-valued pattern or replacement executes
  with row-runtime per-row literal-pattern semantics inside native
  orchestration. Parity bar: per-row pattern resolution, empty
  pattern behavior, and null propagation match the row engine.
  Retires `DataDependentReplacePattern` and completes the `string
  functions` row.

- Complete conditional function native coverage.

  Status: Shipped in 0.49.0.

  `if_else(condition, when_true, when_false)` currently lowers only
  when the condition and branch expressions are native-supported and
  the typed engine can produce one compatible branch dtype. v0.49
  must also cover mixed-class branch outputs and row-runtime branch
  selection semantics. The lowering may reuse the tagged value
  representation introduced for mixed-class `pivot_longer`, dynamic
  `col`, and null-padding union, but the representation must remain
  invisible to downstream stages and direct writers. Parity bar:
  boolean condition validation, null-condition output, selected-branch
  evaluation, mixed numeric/string/boolean/null branch classes, and
  downstream rendering match the row engine byte-for-byte. Completes
  the `conditional functions` row.

- Promote mixed-class `pivot_longer` value columns to native.

  Status: Shipped in 0.49.0.

  Mixed-class value column sets execute through native orchestration
  with row-runtime per-cell value classes at the reshape boundary.
  Parity bar: interleaved output order, per-cell class preservation
  through downstream stages, and sink bytes match the row engine.
  If the compatibility bridge cannot reach byte parity for some
  consumer, that consumer is a release blocker to fix, not a
  carve-out.
  Completes the `pivot_longer` row.

- Promote `complete` class-changing and window-bearing fills to
  native.

  Status: Shipped in 0.49.0.

  With temporal and window promotion landed, fill expressions are
  accepted by native planning and class-changing fills evaluate with
  row-runtime value classes at the key-expansion boundary. Parity
  bar: expansion order, fill evaluation order, and class outcomes
  match the row engine. Completes the `complete` row.

- Promote the remaining window subsets to native.

  Status: Shipped in 0.49.0.

  Two residues flip: (a) a single assignment combining windows from
  incompatible composite order groups lowers each window in its own
  `over` context with per-window `sort_by`, removing the
  one-group-per-assignment restriction; (b) `lag` / `lead` with
  expression offsets or non-literal defaults lower via
  partition-local index arithmetic (`int_range` + `gather`) with
  default fill. Parity bar: edge truncation, all-null order keys,
  and tie handling match the row engine, mirroring the v0.48
  bounded-frame parity corpus. `WindowExpression` retires from
  runtime planner output; the `window offset functions` row and the
  `incompatible multi-key window order groups` row flip to
  `native parity`.

- Complete the inherited stage and pipeline-shape rows.

  Status: Shipped in 0.49.0.

  Once every expression family is native, the partial qualifiers on
  `mutate`, `group_by`, `agg`, `join`, `union`, `load`, context
  references, conditional functions after the dedicated `if_else`
  promotion, aggregate arguments, named bindings, binding-start
  pipelines, and named-output programs dissolve: their row-only
  subsets were all inherited from expression families promoted above.
  This Must is the sweep that
  flips each of those rows to unqualified `native parity`, deletes
  the `aggregate arguments over row-only expressions` and
  `out-of-contract numeric coercions` rows (the latter described a
  hypothetical that never shipped), confirms `BindingStartNotEligible`
  and `NamedOutputMixedEngines` no longer occur for language-feature
  reasons, and confirms the PDL-to-Algraf Arrow IPC handoff row as
  `native parity`. Remote / object-store paths are not part of this
  sweep: they are not a shipped language feature and stay out of
  scope (see Non-Goals).

- Flip the coverage matrix to the two-value status vocabulary.

  Status: Shipped in 0.49.0.

  `docs/PDL_NATIVE_COVERAGE.md` header drops `native partial` and
  `planned native` from the documented status set.
  `docs/PDL_NATIVE_COVERAGE.csv` status column is restricted to
  `native parity` and `row-only by design`. After the promotions
  above, exactly two rows carry `row-only by design`: `host/WASM`
  and `host/LSP/editor`. The matrix update is a single commit
  alongside the eligibility-CSV agreement test.

- Implement the CSV-as-oracle eligibility test.

  Status: Shipped in 0.49.0.

  The `pdl-exec` native coverage test reads
  `PDL_NATIVE_COVERAGE.csv` at compile time (via `include_str!`),
  parses each row, and asserts the v0.49 vocabulary and row-only set:
  every status is `native parity` or `row-only by design`, and the
  only row-only rows are `host/WASM` and `host/LSP/editor`. Planner
  acceptance for shipped language-feature examples is covered by the
  parity harness, which now runs `native-strict` for every example
  pinned to native.

- Add `--engine native-strict`.

  Status: Shipped in 0.49.0.

  Flag handling in `crates/pdl-cli/`. The mode errors if any side
  pipeline (binding, named output, non-terminal save) falls back to
  the row engine. The per-output observability surface from v0.48
  makes the side-pipeline check straightforward: walk the per-output
  `selected_engine` fields and fail if any equals `Row`.
  The exit code matches today's `--engine native` ineligibility
  exit code. After this release the flag should never fire on a
  language-feature pipeline; it exists to keep regressions loud.

- Retire promoted `NativeUnsupportedReason` variants.

  Status: Shipped in 0.49.0.

  `TemporalFunction`, `NonEquiJoin`, `UnionNullPadding`,
  `DataDependentColIndirection`, `DataDependentReplacePattern`,
  `UnsupportedNumericCoercion`, `WindowExpression`,
  `StdinBytesBackedScan`, and `HostBytesBackedScan` leave the
  runtime vocabulary, or become unreachable for valid
  language-feature pipelines if keeping the enum variants avoids a
  churny public JSON break. The latter two were already retired from
  planner output in v0.46 and slated for v0.49 deletion.
  `InputFormat` survives only for genuinely unknown formats, not JSON
  Lines. `ScalarFunction`, `ScalarFunctionArity`,
  `AggregateFunction`, `AggregateArity`, `RowOnlyStage`,
  `BindingStartNotEligible`, `NamedOutputMixedEngines`,
  `NonTerminalSaveFanout`, `NativeSinkWriter`, `DriverFacts`, and
  `NoRunnableMain` survive as defensive, invalid-program, or
  observability boundaries that the CSV-as-oracle test asserts are
  unreachable for native-parity language-feature cells. In
  particular, empty `pivot_longer` column lists, empty `complete` key
  lists, unsupported-stage parser recovery, arity errors, and missing
  driver facts are diagnostics or defensive paths, not row-only
  coverage cells. `WasmTargetGraph` and `EditorService` survive as
  documentation-only host boundaries.

- Hold the WASM target graph.

  Status: Shipped in 0.49.0.

  `pdl-wasm` Cargo manifest is unchanged. `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` remains green. No promotion in
  this release introduces a wasm-reachable Polars, Arrow, or
  Parquet dependency.

- Update the spec, examples, and release stamps.

  Status: Shipped in 0.49.0.

  `docs/PDL_SPEC.md` records the v0.49 history line, rewrites the
  native-execution section to state that every language feature is
  native-eligible, documents `--engine native-strict`, and notes
  the two-value status vocabulary. Each promotion adds or extends a
  runnable example (temporal pipeline, JSON Lines load,
  null-padding union, dynamic col/replace, mixed-branch
  `if_else`, mixed-class `pivot_longer`, window residues). Every
  example continues to round-trip identically on both engines through
  the parity harness. Workspace `Cargo.toml`, `Cargo.lock`,
  `editors/vscode/package.json`,
  `editors/vscode/package-lock.json`, and any demo manifests bump
  to `0.49.0`. NPM consumer pins follow `AGENTS.md` "NPM package
  version checks".

## Should

- Land promotions in dependency order, one commit per Must.

  Status: Completed for the release as a consolidated implementation.

  Suggested order: temporal functions first (unblocks aggregates,
  windows, group_by keys, complete fills); JSON Lines sources;
  union null-padding; dynamic col and dynamic
  replace; mixed-branch `if_else`; window residues; mixed-class
  pivot_longer and complete fills (hardest parity work, benefits from
  everything before it); the inherited-row sweep; CSV-as-oracle test;
  status vocabulary cleanup; `--engine native-strict` last. Each
  commit passes
  `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets`, `cargo test --workspace`, and the parity harness
  independently, and carries its `selected_engine` fixture flips
  with a plan-section reference per the fixture update protocol.

- Add a CI gate that runs `--engine native-strict` over the full
  example set.

  Status: Shipped in 0.49.0 via the parity harness. The existing CI
  `cargo test --workspace` step runs the example parity tests, and
  the harness now adds a `native-strict` leg for every example whose
  `selected_engine` fixture is `native`.

  With every language feature promoted, the strict gate widens from
  a curated subset to all runnable examples: none may demote, even
  partially. Distinct from the parity harness, which proves byte
  parity; strict proves engine selection.

- Benchmark the promoted paths.

  Status: Shipped in 0.49.0.

  Extend `crates/pdl-bench` workloads to cover temporal-heavy,
  JSON-Lines-heavy, dynamic column/text, expression-offset window,
  and null-padding union pipelines at the million-row scale,
  recording row-vs-native ratios in `bench/README.md`. Non-equi join
  benchmarking is omitted because no non-equi join syntax is shipped.
  The promotions exist to be fast; the bench table is the receipt.

- Document the post-v0.49 row-only catalog.

  Status: Shipped in 0.49.0.

  After v0.49 the row-only set is exactly the two host boundaries.
  Add a short subsection to `PDL_SPEC.md` (or a sibling
  `docs/ROW_ONLY_CATALOG.md`) stating that, with the WASM and
  LSP/editor rationale. This is documentation of the closed arc,
  not policy.

## Could

- Surface a `--engine row-strict` flag for CI parity.

  Status: Already shipped in 0.43.0. No v0.49 work remains;
  `--engine native-strict` in this release completes the symmetric
  strict pair.

- Evaluate configurable CSV dialect support.

  Status: Resolved in 0.49.0 as future scope; carried to
  `V0_50_PLAN.md`.

  The evaluation did not identify a hidden row-only native coverage
  gap. CSV dialects would add new source/sink contract surface, so
  they belong in a later release thesis rather than the v0.49 parity
  closeout.

- Evaluate a browser byte IO ABI for binary host-file contents and
  Arrow IPC output.

  Status: Resolved in 0.49.0 as future scope; carried to
  `V0_50_PLAN.md`.

  The current WASM ABI is intentionally row-only and in-memory. A
  browser byte IO expansion would be an ABI feature, not a native
  promotion, and therefore stays outside v0.49.

- Evaluate object-store and remote path support with a dedicated
  security and IO plan.

  Status: Resolved in 0.49.0 as future scope; carried to
  `V0_50_PLAN.md`.

  Remote paths are not a shipped language feature; nothing here is a
  row-only cell to promote. Any future object-store work needs a
  security and IO plan before it becomes implementation scope.

## Risks

- Mixed-class `pivot_longer` is the riskiest promotion: the native
  orchestration boundary must preserve every downstream stage and
  sink's row-visible value classes. If parity proves unreachable
  there, the release holds rather than shipping a carve-out — the
  thesis of v0.49 is zero language-feature fallbacks, and a single
  exception reintroduces the intermediate-status vocabulary this
  release deletes.
- JSON Lines inference parity requires preserving the row reader's
  schema and missing-field semantics. The shipped implementation uses
  the row reader at the native orchestration boundary rather than a
  separate NDJSON inference contract. Acceptable: correctness first,
  and the selected engine remains native.
- Non-equi join ordering is not a shipped v0.49 risk because PDL has
  no non-equi join syntax. The reserve row was retired from the
  coverage matrix instead of introducing syntax in a no-language-surface
  release.
- Mixed-branch `if_else` is a semantic trap because the row runtime
  evaluates only the selected branch and may preserve different value
  classes on different rows. The native path must prove both
  branch-selection behavior and downstream row-visible rendering
  before the `conditional functions` row can flip.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Parity harness (must pass green at every commit on the v0.49 branch):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Coverage matrix consistency (the CSV is now the compile-time oracle):

```bash
cargo test -p pdl-exec native_coverage
# Asserts: every `native parity` row in PDL_NATIVE_COVERAGE.csv is
# accepted by the eligibility check; every `row-only by design` row
# is rejected with the declared NativeUnsupportedReason variant.
# After v0.49 the row-only set is exactly host/WASM and
# host/LSP/editor.
```

`--engine native-strict` over the example set:

```bash
for f in examples/*.pdl; do
  cargo run -p pdl-cli -- run "$f" --engine native-strict || exit 1
done
# Expected: every example runs native end to end, no fallback.
```

WASM target graph audit:

```bash
cargo tree -p pdl-wasm --target wasm32-unknown-unknown | grep -E 'polars|arrow|parquet'
# must be empty
```

## Non-Goals

- Do not promote WASM or the LSP/editor surface to native. Those
  boundaries are `row-only by design` and stay so. Promoting WASM
  would require Polars in the wasm target graph, which the WASM
  contract forbids.
- Do not remove the row engine. It remains the parity reference, the
  WASM execution path, and the `--engine row` opt-in.
- Do not introduce Polars, Arrow, or Parquet into the `pdl-wasm`
  dependency graph.
- Do not change CSV, JSON Lines, Arrow IPC, or Parquet output bytes
  on the row engine. The row writer is the spec; native promotions
  conform to it, never the reverse.
- Do not introduce new PDL language surface, new stage keywords, or
  new functions.
- Do not ship a promotion below byte parity. No "fast but slightly
  different" mode, no per-feature parity waivers.
- Do not reintroduce `native partial` or `planned native` as
  matrix statuses for future releases. After v0.49 the vocabulary
  is closed.
- Do not silently demote any pipeline that runs natively today.
- Do not introduce object-store, network, or remote-path support;
  that is a separate plan with its own security model.

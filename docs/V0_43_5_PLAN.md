# PDL v0.43.5 Plan

Status: Implemented (shipped as 0.43.5)
Target version: 0.43.5 (the plan's original `0.43.5.0` stamp is normalized to three-segment semver for Cargo/npm)
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](https://www.google.com/search?q=PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](https://www.google.com/search?q=PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](https://www.google.com/search?q=PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_43_PLAN.md`](https://www.google.com/search?q=V0_43_PLAN.md)
Successor plan: [`V0_44_PLAN.md`](https://www.google.com/search?q=V0_44_PLAN.md)

## Purpose

PDL v0.43.5 replaces the SQL-shaped window-frame clause with a named-frame surface. The existing `rows between <bound> and <bound>` form is removed from the language. It is the only place in PDL source where modifiers use SQL connective grammar (`between`, `and`) rather than the `snake_case_keyword <argument>` pattern used by every other window-spec clause (`partition_by segment`, `order_by score desc`) and every other multi-word concept in the language (`partition_by`, `pivot_longer`, `nulls_first`, `current_row`). v0.43.5 closes the inconsistency.

**This is a breaking change.** Every `.pdl` source file using `rows between ... and ...` MUST be rewritten before it compiles under v0.43.5. There is no deprecation cycle, no parallel surface, and no compatibility flag.

The new frame clause is `frame <name> [N]`, with six legal names:

* `frame whole_partition` — every row in the partition
* `frame running` — start of partition through the current row
* `frame remaining` — current row through end of partition
* `frame trailing N` — last `N` rows plus the current row
* `frame leading N` — current row plus next `N` rows
* `frame centered N` — `N` rows before the current row, the current row, and `N` rows after

The named forms are the only legal frame surface. Asymmetric or arbitrary-bound frames (e.g. `2 preceding and 5 following`) are not expressible in v0.43.5 source. They can be reintroduced later through a named-frame extension (e.g. `frame window before: N after: M`) when an actual need arises; until then, the closed vocabulary is the spec.

The release ships the two named frames the native engine already supports — `whole_partition` and `running` — at full native parity. The four bounded names (`trailing`, `leading`, `centered`, `remaining`) parse, pass semantics, and execute on the row engine; native execution rejects them through the existing bounded-frame `NativeUnsupportedReason` variant until bounded-frame native lowering arrives in a later release.

The release is gated by v0.43 (the closed `native parity` / `row-only by design` vocabulary). It is the first release in the v0.43–v0.49 arc that intentionally breaks source compatibility. No external `.pdl` corpus is assumed to exist; every legacy occurrence inside the repository is rewritten in the same PR, and the parser offers no tailored migration diagnostic for the removed syntax — users encountering legacy source will see a generic parse error, which is acceptable given the absence of an external user base.

## Implemented Scope

Two rules hold at every commit:

1. **Single surface**: After v0.43.5, `WindowFrame` has exactly one production: `frame <name> [N]`. No `rows`, no `between`, no `and`, no `preceding`, no `following`, no `unbounded_preceding`, no `unbounded_following`, no `current_row` appear in the surface grammar. The parser, formatter, editor services, and spec all reflect the single form.
2. **WASM stays Polars-free**: The named-frame surface lands in `pdl-syntax`, `pdl-semantics`, and `pdl-editor-services`. `pdl-wasm` target graph is unchanged.

Every example, fixture, test, and snippet in the repository that uses the legacy form is rewritten in the same release. The repository contains no legacy frame syntax after v0.43.5 ships.

## Replacement Scope

### Grammar

`WindowFrame` becomes a single production:

```ebnf
WindowFrame  ::= "frame" FrameName FrameArg? ;
FrameName    ::= "whole_partition"
               | "running"
               | "remaining"
               | "trailing"
               | "leading"
               | "centered" ;
FrameArg     ::= IntLiteral ;

```

`whole_partition`, `running`, and `remaining` MUST NOT take an integer argument. `trailing`, `leading`, and `centered` MUST take a single non-negative `IntLiteral` argument. A zero argument is legal and collapses the named frame to the obvious degenerate case (`frame trailing 0` is the current row only); `frame trailing 0`, `frame leading 0`, and `frame centered 0` are useful primarily as parameterized-pipeline degenerate cases.

The `RowsFrame`, `FrameBound`, and the keywords `rows`, `between`, `unbounded_preceding`, `current_row`, `unbounded_following`, `preceding`, `following` are removed from the grammar.

### Reserved words

`PDL_SPEC.md` section 6.5 reserved-words list changes in two passes within the same commit:

**Removed:**

* `rows`
* `between`
* `unbounded_preceding`
* `current_row`
* `unbounded_following`
* `preceding`
* `following`

**Added:**

* `frame`
* `whole_partition`
* `running`
* `remaining`
* `trailing`
* `leading`
* `centered`

The seven removed words are no longer reserved at all; users can name columns `rows`, `current_row`, `following`, etc., without backticks. This is the secondary, smaller payoff of the surface cleanup.

### Diagnostics

v0.43.5 reserves new `E12xx` codes for the named-frame surface:

* `E1230` — unknown frame name in `frame <ident>` clause; suggests the six legal names by Levenshtein distance.
* `E1231` — `frame trailing` / `frame leading` / `frame centered` missing required integer argument.
* `E1232` — `frame whole_partition` / `frame running` / `frame remaining` followed by an unexpected integer argument.

The parser does not reserve a dedicated diagnostic for the removed `rows between ... and ...` syntax. With the seven legacy keywords demoted to ordinary identifiers, source containing the old form produces a generic parse error at whichever token the parser cannot match against the new `WindowFrame` production. That is acceptable: no external corpus is assumed to exist, and in-repo source is rewritten in the same release.

The native-execution diagnostic for bounded frames is unchanged in meaning; its text is updated to name the v0.43.5 surface forms (e.g., "the `frame trailing N` / `frame leading N` / `frame centered N` / `frame remaining` clauses are not supported by native execution").

### IR

`WindowFrameIr` and `FrameBoundIr` in `crates/pdl-semantics/src/ir.rs` (lines 303-316) remain unchanged. The named-frame surface desugars at parse time into the existing bound pairs:

| Surface | Lowered `WindowFrameIr` |
| --- | --- |
| `frame whole_partition` | `unbounded_preceding..unbounded_following` |
| `frame running` | `unbounded_preceding..current_row` |
| `frame remaining` | `current_row..unbounded_following` |
| `frame trailing N` | `N preceding..current_row` |
| `frame leading N` | `current_row..N following` |
| `frame centered N` | `N preceding..N following` |

Keeping the IR fixed minimizes blast radius. The planner, executor, native-coverage rows, editor services, and serialized analyzer outputs see no schema change. A future plan may restructure `WindowFrameIr` into the six named variants for internal clarity, but that is a separable refactor and out of scope for v0.43.5.

Spans on the synthesized `FrameBoundIr` nodes point at the surface `frame` clause so diagnostics, hovers, and editor navigation point at real source.

### Native coverage

* `frame whole_partition` is `native parity` (covered by the existing `DataWindowFrame::WholePartition` arm).
* `frame running` is `native parity` (covered by `DataWindowFrame::UnboundedPrecedingToCurrentRow`).
* `frame remaining`, `frame trailing N`, `frame leading N`, and `frame centered N` are `row-only by design` for v0.43.5, sharing the existing bounded-frame `NativeUnsupportedReason` variant.

The coverage matrix replaces any window-frame rows referencing the legacy `rows between ... and ...` surface with rows referencing the named forms. The CSV column for surface naming uses the named form verbatim; the eligibility test from v0.43 continues to assert agreement between the matrix and the planner.

### Editor services

* TextMate grammar in `editors/vscode/syntaxes/pdl.tmLanguage.json` removes the seven retired keywords from the window-clause scope and adds the seven new ones.
* `pdl-editor-services` completion proposes the six frame names after a `frame ` token inside a `WindowSpec`. Hover surfaces the bound pair the named form lowers to so power users understand the underlying frame.
* The formatter (`pdl fmt`) emits only the named form. There is no formatter migration mode; `pdl fmt` operating on legacy source surfaces the same parse error the compiler does and exits non-zero.

### Repository sweep

Every legacy frame occurrence inside the repository is rewritten in the same release:

* Every example under `examples/`.
* Every fixture and snapshot under `tests/` and per-crate `tests/` directories.
* Every `.pdl` snippet inside `docs/`, plan files, and README files.
* Every benchmark workload under `bench/`.

The repository contains no legacy frame syntax after the v0.43.5 PR merges. A CI grep guard locks the cleanup in (Should, below).

## Must

* **Replace the `WindowFrame` grammar with the named-frame production.**
Status: Implemented.
Rewrite `parse_window_frame` in `crates/pdl-syntax/src/parser.rs` (around line 1711). The new entry point parses `frame`, a frame-name identifier, and an optional `IntLiteral`. Unknown names raise `E1230`. Arity validation raises `E1231` / `E1232`. `parse_frame_bound` (around line 1732) is removed. The seven legacy keywords are removed from the lexer's reserved set; source containing them produces a generic parse error at the unmatched token.
* **Rewrite `PDL_SPEC.md` window-expression sections.**
Status: Implemented.
Section 6.5 reserved-words list drops the seven retired words and adds the seven new ones in the same commit. The window-expression section (currently around line 2030) is rewritten to introduce the six named frames and the mapping table. The grammar production in section 11 (currently around line 1358) is replaced with the EBNF above. The v0.26 normative statement asserting that `rows between unbounded_preceding and current_row` MUST remain valid v0.26 syntax is retracted in v0.43.5; the spec history line records the retraction. Backticked column-name carve-outs for the seven removed words are no longer needed and are deleted.
* **Rewrite every `.pdl` source file and embedded snippet in the repository.**
Status: Implemented.
Sweep `examples/`, `tests/`, `docs/`, `bench/`, plan files, and READMEs for legacy frame syntax and rewrite each occurrence to the equivalent named form. Implementation note: by maintainer decision, historical plan documents (`V0_16`–`V0_39`) keep the surface wording of the releases they describe; the no-hits guarantee is scoped to `*.pdl` and `*.rs`. The normative docs (`PDL_SPEC.md`, `PDL_NATIVE_COVERAGE.*`, READMEs) carry only the named forms.
* **Hold IR stability.**
Status: Implemented.
`WindowFrameIr` and `FrameBoundIr` in `crates/pdl-semantics/src/ir.rs` (lines 303-316) are unchanged. A test asserts each named-frame surface form produces an IR bit-identical to a constructed reference IR. Internal IR consumers (planner, native lowering, row execution, editor services, serialized analyzer outputs) compile without changes beyond diagnostic-text updates.
* **Native parity for `frame whole_partition` and `frame running`.**
Status: Implemented.
`crates/pdl-exec/src/runtime/native_lowering.rs` `lower_data_window_frame` (around line 411) requires no logic change because the parser desugars into the same `FrameBoundIr` pairs the existing match arms accept. A parity test executes a pipeline using each of the two parity-eligible named frames on both engines and diffs output bytes. `selected_engine` fixtures for the new examples record `NativePolars`.
* **Bounded named frames execute on the row engine; the native engine rejects them with the updated diagnostic.**
Status: Implemented.
`frame trailing N`, `frame leading N`, `frame centered N`, and `frame remaining` desugar to the bounded `FrameBoundIr` pairs the native engine already rejects. The native-engine diagnostic text is updated to name the v0.43.5 surface forms; the variant code is unchanged. Row-engine semantics are unchanged from v0.43.
* **Replace editor assets and completion.**
Status: Implemented.
TextMate grammar in `editors/vscode/syntaxes/pdl.tmLanguage.json` drops the seven retired keywords and adds the seven new ones. `pdl-editor-services` completion proposes the six frame names after a `frame ` token inside a `WindowSpec`. Hover surfaces the bound pair the named form lowers to.
* **Hold the WASM target graph.**
Status: Implemented.
`pdl-wasm` Cargo manifest is unchanged. `cargo check -p pdl-wasm --target wasm32-unknown-unknown` remains green. The named-frame surface adds no wasm-reachable dependencies.
* **Update release stamps and consumer pins.**
Status: Implemented.
Workspace `Cargo.toml`, `Cargo.lock`, `editors/vscode/package.json`, `editors/vscode/package-lock.json`, and any demo manifests bump to `0.43.5.0`. `docs/PDL_SPEC.md` history line records the v0.43.5 breaking change with a one-line summary. NPM consumer pins follow `CLAUDE.md` "NPM package version checks". Implementation note: because the named-frame surface changes the WASM parser and editor grammar the browser packages carry, `pdl-wasm@0.43.5` and `pdl-editor@0.43.6` are prepared locally for publication; consumer pins (demo, lockfile `node_modules` entries, install docs) stay on the latest verified published `0.39.0` until npm confirms the new tarballs exist. `docs/NPM_PACKAGES.md` records the gap.

## Should

* **Land the grammar plus source rewrite, IR-stability test, spec, editor assets, and release stamps in separate commits.**
Status: Implemented.
Suggested order: grammar + parse-time desugaring + repository source rewrite + IR-stability test in a single commit (the grammar flip and the source rewrite are atomic; either lands together or every in-repo `.pdl` example fails to parse); spec rewrite second (purely docs, but follows the code so the spec reflects shipped behavior); editor assets third; release stamps last. Each commit passes `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and the parity harness independently.
* **Add a CI grep guard against the legacy frame syntax.**
Status: Dropped by maintainer decision; no CI guard step ships with v0.43.5. The repository-wide cleanup is still verified by the test suite (the legacy surface no longer parses), and historical plan documents intentionally retain pre-v0.43.5 wording.
A CI step runs `git grep -nE '\brows[[:space:]]+between\b' -- '*.pdl' '*.rs'` and fails on any hit. The guard locks in the cleanup and prevents accidental reintroduction. Markdown is excluded because historical plan documents intentionally keep the pre-v0.43.5 wording.
* **Add parity-corpus rows for the named frames.**
Status: Implemented.
Two new parity rows assert byte-identical row-vs-native output for `frame whole_partition` and `frame running`. Four new row-engine-only fixtures assert row-engine success and native-engine rejection for `frame trailing N`, `frame leading N`, `frame centered N`, and `frame remaining`.
* **Add formatter snapshots for each named frame.**
Status: Implemented.
`pdl fmt` snapshots round-trip each of the six named forms with stable formatting (single-line vs multi-line shape rules follow the existing `over (...)` clause heuristics).

## Could

* **Promote bounded named frames (`trailing N`, `leading N`, `centered N`, `remaining`) to native parity.**
Status: Deferred.
Requires bounded-window-frame lowering in `crates/pdl-exec/src/runtime/native_lowering.rs`. Tracked alongside the existing bounded-frame native gap. Out of scope for v0.43.5.
* **Default the frame for `first_value` and `last_value` over an ordered partition to `whole_partition`.**
Status: Deferred.
Would close a well-known SQL footgun: with no explicit frame, `last_value(col) over (order_by ...)` returns the current row rather than the last value in the partition. The fix is small in the IR default-frame resolution but diverges from SQL semantics for users porting queries. Treat as a separate proposal with its own discussion of default-frame semantics across `first_value`, `last_value`, and any future value-window functions.
* **Restructure `WindowFrameIr` into six named variants.**
Status: Deferred.
v0.43.5 keeps the bound-pair IR to minimize blast radius. A later refactor could replace `WindowFrameIr { start, end }` with an enum whose variants mirror the surface (`WholePartition`, `Running`, `Trailing { rows }`, etc.). The change is a pure internal cleanup and a single-commit refactor across the planner, executor, editor services, and analyzer outputs.
* **Reintroduce arbitrary-bound frames through a named extension.**
Status: Deferred.
If a real workload needs asymmetric bounds (e.g. `2 preceding..5 following`), a future plan could add `frame window before: N after: M` or similar. The closed v0.43.5 vocabulary is intentional; expanding it requires a proposal with a motivating use case.
* **Ship a `pdl migrate` subcommand that rewrites a `.pdl` file in-place.**
Status: Deferred.
Not needed for the in-repo sweep; the repository rewrite is small enough to do by hand. If an external user with a non-trivial legacy corpus appears later, a `pdl migrate v0_50_frames` subcommand could be written then as a one-off helper. Out of scope for v0.43.5.

## Implementation Notes

* **Planner eligibility fix (shipped with this release).** `native_pipeline_unsupported_reason` in `crates/pdl-exec/src/planning.rs` used `?` on the per-item `Option<NativeUnsupportedReason>` results for `mutate` and `agg` stages, which discarded unsupported-item reasons and declared the pipeline native-eligible at the first supported item. Bounded window frames therefore planned as `selected_engine: native` and silently fell back to the row engine at native-lowering time. The loops now return the first unsupported reason, so the four bounded named frames plan as `row` with the `window-expression` reason, in agreement with the coverage matrix. No committed `selected_engine` fixture changed value as a result of this fix.
* The surface AST in `pdl-syntax` now carries the named frame (`WindowFrame { kind: WindowFrameKind }`); desugaring to the unchanged `WindowFrameIr` bound pairs happens in `pdl-semantics` lowering. `pdl ast` JSON renders the named form (`{"name": ..., "rows": ...}`); `pdl ir` JSON is unchanged.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace

```

Named-frame surface tests:

```bash
cargo test -p pdl-syntax window_frame_named
cargo test -p pdl-semantics window_frame_named_ir_stability
# Asserts: each `frame <name> [N]` surface form produces a
# WindowFrameIr bit-identical to the equivalent reference IR.

```

Repository-wide legacy-syntax guard:

```bash
git grep -nE '\brows[[:space:]]+between\b' -- '*.pdl' '*.md' '*.rs'
# must return no hits

```

End-to-end smoke on the rewritten examples:

```bash
cargo run -p pdl-cli -- check examples/window_frame_named.pdl
cargo run -p pdl-cli -- run examples/window_frame_named.pdl
cargo run -p pdl-cli -- run examples/window_frame_named.pdl --engine native
# `frame whole_partition` and `frame running` examples run native.
# Bounded named frames error with the updated bounded-frame native
# diagnostic on `--engine native`.

```

WASM target graph audit:

```bash
cargo tree -p pdl-wasm --target wasm32-unknown-unknown | \
  grep -iE 'polars|arrow|parquet'
# must be empty

```

VS Code extension checks:

```bash
cd editors/vscode
npm install
npm run lint
npm run test
npm run package

```

## Non-Goals

* Do not keep the `rows between ... and ...` clause as a parallel or deprecated surface. v0.43.5 is a clean cut with no legacy remnant.
* Do not introduce a `--legacy-frames` flag, a feature gate, or any other mechanism that lets users opt in to the removed syntax.
* Do not implement a tailored diagnostic for the removed syntax. Source containing `rows between ... and ...` produces a generic parse error; that is acceptable given the absence of an external user base.
* Do not change the IR. `WindowFrameIr` and `FrameBoundIr` stay exactly as they are; a future refactor may restructure them, but not in v0.43.5.
* Do not promote bounded window frames to native parity. The existing bounded-frame native gap is unchanged; the four bounded named frames inherit it.
* Do not change the default frame for `first_value`, `last_value`, or any other window function. SQL's "current row" default for value functions remains v0.43.5 behavior; changing it is a separately tracked proposal.
* Do not introduce `range between ...` or any `range`-flavored named frame. PDL is still `rows`-only at the window-frame layer; a `range` extension is its own future proposal.
* Do not introduce silent rewriting in `pdl fmt`. The formatter produces named forms only; legacy input fails the parse before the formatter sees it.
* Do not introduce arbitrary-bound frames through any v0.43.5 surface. The closed six-name vocabulary is the spec; widening it requires a separate proposal with a motivating use case.
* Do not remove or rename window functions (`lag`, `lead`, `first_value`, `last_value`, `row_number`, `rank`, `dense_rank`, `percent_rank`, `cume_dist`, `sum`, `mean`, `max`, `min`, `count`). v0.43.5 changes only the frame clause.
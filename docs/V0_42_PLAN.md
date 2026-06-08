# PDL v0.42 Plan

Status: Proposed
Target version: 0.42.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_41_PLAN.md`](V0_41_PLAN.md)
Neighboring Algraf plan: [`V0_74_PLAN.md`](../../algraf/docs/V0_74_PLAN.md)

## Purpose

PDL v0.42 is an internal maintenance release. It restructures three oversized
files in the runtime, editor-services, and CLI crates without changing the
language surface, the row/native parity contract, diagnostics, public crate
APIs, output bytes, or test counts. The goal is to make the v0.41 language
work (union schema extension, non-equi joins) and subsequent native-coverage
expansion easier to land by reducing the per-change reading cost in code paths
that today fold five or six unrelated concerns into single modules.

Three files dominate that cost:

- `crates/pdl-exec/src/runtime.rs` is 6,699 lines and folds row-runtime
  evaluation, window evaluation, stage transformations (`pivot_longer`,
  `complete`, join/union helpers), native eligibility checks, native
  lowering, and shared comparison utilities into one module built around
  `impl Runtime<'_>` plus a long tail of free functions.
- `crates/pdl-editor-services/src/services.rs` is 2,482 lines and exposes
  the editor-services public API alongside `CompletionContext` construction,
  `DocumentFacts` extraction, schema inference, and rename support, with the
  same column-scope analysis re-done across several providers.
- `crates/pdl-cli/src/render.rs` is 1,928 lines and interleaves schema,
  plan, manifest, AST, and IR rendering across `text` and `json` outputs,
  with parallel struct hierarchies for AST-to-JSON and IR-to-JSON.

v0.42 splits each file into focused submodules without renaming public items,
moving tests, or shifting behavior. PDL_SPEC.md does not document internal
module layout, so no spec text changes; the spec gains only the v0.42 history
line and version stamp when the plan ships.

## Implemented Scope

This is a code-organization release. The user-visible language surface, the
on-disk and on-stdout output bytes, and every diagnostic code remain
identical to v0.41. Public function signatures, struct field visibility,
error variants, and crate boundaries are preserved. Inline `#[cfg(test)]`
modules move with the functions they cover; the workspace test count is
conserved.

The split obeys three rules at every commit:

1. Behavior preservation. `cargo test --workspace` output (including
   snapshot tests) matches v0.41 byte-for-byte. End-to-end byte parity of
   `pdl run|check|schema|plan|manifest|ast|ir` outputs is required.
2. No cross-crate API change. Splits happen inside each crate. Internal
   `pub(crate)` visibility replaces in-file private visibility where new
   modules need to reach across. `lib.rs` re-exports are unchanged.
3. WASM target graph is unchanged. `pdl-wasm` remains free of Arrow,
   Parquet, and Polars. The split inside `runtime.rs` does not introduce
   new feature flags or default-on dependencies.

## Refactor Scope: `crates/pdl-exec/src/runtime.rs`

Current shape: one `impl Runtime<'_>` block (~575 lines) plus ~6,100 lines of
free functions split across native execution, row evaluation, and stage
operations. Target shape: a thin `runtime.rs` retaining the `Runtime` struct,
the `Runtime` impl with the pipeline-stage dispatch methods (`execute_pipeline`,
`filter`, `aggregate`, `mutate`, `join`, `union`, `execute_load`,
`execute_binding`, `execute_save`), and four sibling submodules under
`crates/pdl-exec/src/runtime/`:

- `runtime/native_lowering.rs` — expression, aggregate, and window
  translation to the `pdl-data` facade:
  `lower_data_expr`, `lower_data_call`, `lower_data_window`,
  `lower_data_window_frame`, `lower_data_window_spec`,
  `lower_data_agg_items`, `lower_data_agg_arg`, `lower_data_mutate_items`,
  `native_static_text_arg`, `value_to_data_literal`.
- `runtime/native_planning.rs` — native eligibility checks and native
  pipeline orchestration:
  `try_execute_native`, `check_native_program_eligibility`,
  `check_native_pipeline_eligibility`, `check_native_load_eligibility`,
  `check_native_save_eligibility`, `execute_native_pipeline`,
  `execute_native_binding`, `execute_native_save`,
  `check_native_mutate_multi_key_window_order_groups`,
  `expr_multi_key_window_order_incompatible`, `native_load_plan`,
  `resolve_native_column_*`, `ir_binding`, plus the supporting
  `NativeBindingRef` and `NativePipelineResult` types.
- `runtime/row_eval.rs` — row-runtime cell and window evaluation:
  `eval_row_expr`, `eval_call`, `round_digits`, `round_value`,
  `eval_window_expr`, `eval_offset_window`, `window_offset`,
  `ordered_partition_indices`, `partition_key`,
  `compare_rows_for_window_order`, `compare_values_for_window_sort`,
  `rank_value`, `dense_rank_value`, `last_peer_position`, `order_key`,
  `frame_indices`, plus the `EvalScope` struct and `ExprRole` enum.
- `runtime/stages.rs` — stage-specific row transformations and
  schema-compatibility checks:
  `pivot_longer`, `complete`, `join_columns`, `right_non_key_indices`,
  `join_index`, `row_join_key`, `row_key`, `combine_rows`,
  `right_only_row`, `join_semi_anti`, `ensure_union_compatible`,
  `ensure_union_column_compatible`, `ensure_key_types_compatible`, plus
  the `CompleteContext` struct and `ValueClass` enum. The pipeline-stage
  `Runtime` methods stay in `runtime.rs` and call into `stages::*` and
  `row_eval::*` free functions.

Cross-cutting borrow considerations: `Runtime<'a>` continues to borrow
`PreparedProgram` and `DriverIo` for the full pipeline. Native-path
functions take `&'a PreparedProgram` and `&'a ExecutionPlan` explicitly.
Row-path functions in `row_eval.rs` take `&Table`, `&WindowSpecIr`, and
`&BTreeMap<String, Value>` context arguments rather than `&self`. No new
lifetimes are introduced. The shared context map continues to thread
through both paths unchanged. All `Result<T, Diagnostic>` flow is
preserved.

## Refactor Scope: `crates/pdl-editor-services/src/services.rs`

Current shape: the editor-services public API (`analyze_document`,
`completions`, `formatting_edit`, `semantic_tokens`, `document_symbols`,
`binding_definition`, `binding_references`, `rename_binding_edits`) plus
`CompletionContext`, `DocumentFacts`, and roughly fifty column, scope, and
schema-inference helpers. Target shape: a thin `services.rs` retaining the
public API surface, plus three sibling submodules under
`crates/pdl-editor-services/src/`:

- `completion.rs` — `CompletionContext` struct and impl (`new`,
  `completion_items`, `stage_name_*`, `context_reference_kind_*`,
  `inside_string_on_line`, `is_ident_char`), plus `stage_keywords`,
  `column_completions`, `binding_completions`, `format_completions`,
  `function_completions`, `join_kind_completions`,
  `sort_direction_completions`.
- `scope_analysis.rs` — `DocumentFacts` struct and impl (`new`,
  `schema_before_offset`, `pipeline_schema_before_offset`,
  `pipeline_schema`, `pipeline_start_schema`, `apply_stage_to_schema`,
  `apply_complete_fill_to_schema`), plus `columns_before_offset`,
  `column_sources`, `select_item_output_names`,
  `resolve_binding_column_names`, `infer_binding_schema`,
  `infer_select_schema`, `infer_mutate_schema`, `infer_agg_schema`, and
  the supporting `SchemaState`, `BindingFact`, `ContextFact` types.
- `symbols_and_refs.rs` — `binding_definition_symbol`,
  `binding_definition_at`, `binding_references_at`,
  `column_rename_candidate`, `rename_candidate_edits`,
  `binding_rename_candidate`, `find_binding_by_name`. The public
  `binding_definition`, `binding_references`, and `rename_binding_edits`
  entry points stay in `services.rs` and thinly delegate.

`semantic_tokens` and the existing `hover.rs` are unchanged. `lib.rs`
re-exports are unchanged.

## Refactor Scope: `crates/pdl-cli/src/render.rs`

Current shape: public formatters for schema, plan, manifest, AST, and IR
mixed with `Serialize`-deriving struct hierarchies and plan/manifest text
helpers. Target shape: a thin `render.rs` retaining the public surface
(`final_schema_columns`, `render_schema_text`, `render_plan_text`,
`schema_json`, `plan_json`, `manifest_json`, `ast_json`, `ir_json`) as
delegating wrappers, plus modules under `crates/pdl-cli/src/render/`:

- `schema_render.rs` — `SchemaJson`, `NamedSchemaJson`, `ColumnJson`,
  `output_schema_json`, and the `SchemaJson` `Serialize` impl.
- `plan_render.rs` — `DriverPlanJson`, `ExecutionPlanJson`,
  `PlanObservabilityJson`, `ExecutionStepJson`, `FormatDecisionJson`,
  `InputJson`, `SinkJson` and the matching builders
  (`driver_plan_json`, `execution_plan_json`, `plan_observability_json`,
  `execution_step_json`, `format_decision_json`), plus the text helpers
  `pipeline_label_text`, `source_text`, `sink_text`,
  `sniffing_reason_text`, `stream_kind_text`, `stream_direction_text`,
  `execution_step_text`. `manifest_json` lands here too because it shares
  the plan-observability structures.
- `ast_serialize.rs` — `ProgramJson`, `BindingJson`, `OutputJson`,
  `PipelineJson`, `StageJson`, `ExprJson`, `WindowSpecJson` and the matching
  `program_json`, `binding_json`, `output_json`, `pipeline_json`,
  `pipeline_start_json`, `source_json`, `sink_ast_json`, `stage_json`,
  `save_stage_json`, `expr_json`, `window_spec_json`, `window_frame_json`,
  `frame_bound_json`, `mutate_item_json`, `complete_fill_item_json`,
  `agg_item_json`, `join_on_json`, `sort_item_json`, `select_item_json`,
  `rename_item_json` converters. The text helpers `sort_direction_text`,
  `unary_op_text`, `binary_op_text`, `context_kind_text` move here.
- `ir_serialize.rs` — `ProgramIrJson`, `BindingIrJson`, `OutputIrJson`,
  `PipelineIrJson`, `StageIrJson`, `ExprIrJson`, and the matching
  `program_ir_json`, `binding_ir_json`, `output_ir_json`,
  `pipeline_start_ir_json`, `stage_ir_json`, `expr_ir_json`,
  `window_spec_ir_json`, `window_frame_ir_json`, `frame_bound_ir_json`,
  `complete_fill_item_ir_json`, `mutate_item_ir_json`, `agg_item_ir_json`,
  `select_item_ir_json`, `rename_item_ir_json`, `sort_item_ir_json`,
  `join_key_ir_json` converters. The `unary_op_ir_text`,
  `binary_op_ir_text`, `join_kind_ir_text`, `context_kind_ir_text` text
  helpers move with them.
- `span_json.rs` — the shared `SpannedJson<T>` generic wrapper used by
  both AST and IR JSON.

## Must

- Split `crates/pdl-exec/src/runtime.rs` into the four submodules under
  `crates/pdl-exec/src/runtime/` described above.

  Status: Proposed.

  Functions move; signatures are preserved. Inline `#[test]` functions
  follow their subjects. `cargo test --workspace` output, including
  snapshot tests, must be byte-identical to v0.41. The `Runtime` struct
  stays in `runtime.rs`; the four sibling files live under
  `crates/pdl-exec/src/runtime/`.

- Split `crates/pdl-editor-services/src/services.rs` into `completion.rs`,
  `scope_analysis.rs`, and `symbols_and_refs.rs`.

  Status: Proposed.

  `services.rs` retains the editor-services public API and thinly
  delegates into the new modules. `lib.rs` exports are unchanged.
  `cargo test --workspace` is byte-identical.

- Split `crates/pdl-cli/src/render.rs` into `schema_render.rs`,
  `plan_render.rs`, `ast_serialize.rs`, `ir_serialize.rs`, and
  `span_json.rs` under `crates/pdl-cli/src/render/`.

  Status: Proposed.

  The public functions stay in `render.rs` as delegating wrappers. CLI
  byte-output for `schema`, `plan`, `manifest`, `ast`, and `ir`
  subcommands is unchanged. CLI integration tests in
  `crates/pdl-cli/tests/` pass without modification.

- Preserve the row runtime as the semantic reference. No native
  eligibility rule changes, no diagnostic code additions or removals.

  Status: Proposed.

  PDL_SPEC.md is not edited beyond its release `Status:` and history line,
  which only move when v0.42 ships. Diagnostic codes keep their
  reservations and texts.

- Hold workspace boundaries.

  Status: Proposed.

  No new public exports across crates. `pdl-core` depends on nothing
  internal; `pdl-data` keeps Polars private; `pdl-semantics` still owns
  type and IR; `pdl-exec` keeps native eligibility behind the same
  facades.

- Keep `pdl-wasm` free of Arrow, Parquet, and Polars.

  Status: Proposed.

  The runtime split does not introduce new feature flags or default-on
  dependencies. `cargo check -p pdl-wasm --target wasm32-unknown-unknown`
  remains green and the target graph excludes Arrow, Parquet, and Polars.

- Bump release version stamps to 0.42.0.

  Status: Proposed.

  Updates: workspace `Cargo.toml`, `Cargo.lock`, `docs/PDL_SPEC.md`
  (`Status:` line and history line), `editors/vscode/package.json` and
  `package-lock.json` (only the local `version` field; consumer
  dependency pins follow AGENTS_PDL.md "NPM package version checks"),
  demo manifest version stamps if present.

## Should

- Land each crate's split in its own commit on the v0.42 branch so the
  diff is reviewable per crate.

  Status: Proposed.

  Suggested order: `pdl-exec` first (largest blast radius, most tests),
  then `pdl-editor-services`, then `pdl-cli`. Each commit must pass
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`,
  and `cargo test --workspace` independently.

- Add a short module-layout comment at the top of `runtime.rs`,
  `services.rs`, and `render.rs` pointing readers at the new sibling
  files.

  Status: Proposed.

  One paragraph each. No spec change, no doc reorganization, no
  duplication of crate-level docs.

## Could

- Extract a small `runtime/compare.rs` for value-class and null-ordering
  helpers if `stages.rs` grows past ~1,500 lines.

  Status: Deferred.

  Decide after the primary split lands. Current value-class logic is
  small enough to stay co-located.

- Consolidate redundant column-scope walks in editor-services into a
  single `EditorAnalyzer` reused across providers.

  Status: Deferred to v0.43.

  The v0.42 split separates the providers; collapsing redundant analysis
  is a behavior-adjacent change worth its own plan and its own performance
  measurement.

- Evaluate configurable CSV dialect support.

  Status: Deferred.

- Evaluate a browser byte IO ABI for binary host-file contents and Arrow
  IPC output.

  Status: Deferred.

- Evaluate object-store and remote path support with a dedicated security
  and IO plan.

  Status: Deferred.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Output-byte parity against v0.41:

```bash
cargo run -p pdl-cli -- check examples/top_regions.pdl
cargo run -p pdl-cli -- run examples/top_regions.pdl
cargo run -p pdl-cli -- schema examples/top_regions.pdl
cargo run -p pdl-cli -- plan examples/top_regions.pdl
cargo run -p pdl-cli -- manifest examples/top_regions.pdl
cargo run -p pdl-cli -- ast examples/top_regions.pdl
cargo run -p pdl-cli -- ir examples/top_regions.pdl
cargo run -p pdl-cli -- run examples/top_regions.pdl --stdout-format arrow-stream > /tmp/out.arrow
```

Each stdout must match the v0.41 reference byte-for-byte. The `plan` and
`manifest` outputs include `selected_engine` decisions; those must not
flip. Editor-services regression is covered by the inline tests in
`crates/pdl-editor-services/src/services.rs` (preserved during the split)
and the LSP integration tests in `crates/pdl-lsp/tests/`.

## Non-Goals

- Do not change the language surface, stage syntax, function set, or
  diagnostic codes.
- Do not change CSV, JSON Lines, Arrow IPC, or Parquet output bytes.
- Do not change native eligibility, `--engine auto` decisions, or
  selected-engine observability.
- Do not introduce new public exports across crates or add feature flags
  to the runtime split.
- Do not introduce Arrow, Parquet, or Polars dependencies into
  `pdl-wasm`, browser packages, the demo runtime, or editor-facing
  browser bundles.
- Do not promote any of the v0.41 deferred items (union schema extension,
  non-equi joins). Those remain v0.41 scope.
- Do not add a new test runner, snapshot framework, or benchmark harness.
  Test colocation is preserved; counts are conserved.

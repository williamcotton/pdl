# PDL v0.46.5 Plan

Status: Shipped
Target version: 0.46.5
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_46_PLAN.md`](V0_46_PLAN.md)
Successor plan: [`V0_47_PLAN.md`](V0_47_PLAN.md)

## Purpose

PDL v0.46.5 promotes the first deterministic temporal scalar
functions. The motivating workload is forensic Git-log comparison:
export commit metadata with repository, hash, author, committer, and
ISO-strict dates, then use PDL to bucket commits by calendar month,
count activity by repository or developer, and emit CSVs suitable for
Excel, pandas, or a plotting script.

The immediate gaps are small but important:

1. Git `--date=iso-strict` emits timestamps such as
   `2025-02-17T14:20:59Z` or
   `2024-01-15T10:22:33-05:00`. PDL needs to parse both forms
   deterministically.
2. Month-level comparison needs a stable `YYYY-MM` key without
   shelling out to Git's `--date=format:%Y-%m` or Python's
   `datetime.fromisoformat`.
3. Repository/developer activity analysis needs calendar extraction
   (`year`, `month`, `day`) over author and committer dates.

This release adds temporal functions only. It does not add Git
integration, plotting, wall-clock functions, time-zone lookup, or new
primitive date/datetime value classes.

## Proposed Scope

### Scalar functions

Add these scalar functions to the row runtime, semantic registry,
editor services, formatter round trips, CLI diagnostics, and spec:

- `date(value)`: parse a date or datetime-like value and return a
  normalized `YYYY-MM-DD` string. Null or unparseable input returns
  null.
- `datetime(value)`: parse an ISO/RFC3339 datetime-like value and
  return a normalized RFC3339 string. Null or unparseable input
  returns null.
- `year(value)`: parse a date/datetime-like value and return the
  four-digit year as a number. Null or unparseable input returns null.
- `month(value)`: parse a date/datetime-like value and return the
  month number `1` through `12`. Null or unparseable input returns
  null.
- `day(value)`: parse a date/datetime-like value and return the day of
  month `1` through `31`. Null or unparseable input returns null.
- `date_floor(value, unit)`: floor a parsed value to `day`, `week`,
  `month`, or `year`. Date inputs return normalized dates. Datetime
  inputs return normalized datetimes at the start of the requested
  unit, preserving the parsed fixed offset. The `week` unit was added
  during the release (maintainer direction) and floors to the ISO
  Monday of the value's week, matching the `%G`/`%V`/`%u` token
  calendar; it produces full week-start dates that downstream temporal
  axes (e.g. Algraf) can parse, unlike the categorical
  `date_format(value, "%G-W%V")` bucket keys.
- `date_format(value, pattern)`: format a parsed value with a small,
  deterministic strftime-style pattern subset. v0.46.5 MUST support
  `%Y`, `%m`, `%d`, `%H`, `%M`, `%S`, `%z`, `%:z`, and `%%`. During
  implementation the scope was extended (maintainer direction) with the
  ISO week tokens `%G`, `%V`, and `%u` plus day-of-year `%j` — all pure
  calendar arithmetic over the already-parsed date, enabling weekly
  bucket keys such as `date_format(author_date, "%G-W%V")`.

The motivating monthly key is:

```pdl
load "developer_commits.csv"
  | mutate commit_month = date_format(author_date, "%Y-%m")
  | group_by repo_name, commit_month
  | agg commit_count = count()
  | sort commit_month, repo_name
  | save "repo_commit_counts_by_month.csv"
```

The richer author/committer export can be analyzed without changing
the source CSV shape:

```pdl
load "developer_commits.csv"
  | mutate
      author_month = date_format(author_date, "%Y-%m"),
      committer_month = date_format(committer_date, "%Y-%m"),
      author_year = year(author_date),
      author_day = day(author_date)
  | group_by repo_name, author_name, author_month
  | agg commits = count()
  | sort author_month, repo_name, author_name
```

### Accepted input forms

The row runtime MUST accept:

- Dates: `YYYY-MM-DD`.
- Datetimes with a `T` separator: `YYYY-MM-DDTHH:MM:SSZ`,
  `YYYY-MM-DDTHH:MM:SS+00:00`,
  `YYYY-MM-DDTHH:MM:SS-05:00`.
- Datetimes with fractional seconds in the same offset forms.

`Z` is a first-class UTC designator. This avoids the Python
`datetime.fromisoformat` portability issue where older Python
versions reject a trailing `Z`.

The parser MAY also accept a space separator
(`YYYY-MM-DD HH:MM:SS+00:00`) if `chrono` accepts it through the
chosen implementation path, but the spec examples and tests use the
forms above.

Temporal parsing is locale-neutral. Month/day names, local time zone
abbreviations, ambiguous short dates such as `02/03/2025`, and
timezone database names such as `America/Chicago` are not accepted in
v0.46.5.

### Value model

v0.46.5 deliberately avoids adding `Value::Date` or
`Value::DateTime`. Temporal functions parse at the function boundary
and return existing PDL value classes:

- normalized date/datetime values return `string`;
- `year`, `month`, and `day` return `number`;
- parse failures return `null`.

This keeps CSV/JSON/Arrow rendering, schema inference, joins, sorting,
and WASM ABI shape stable. Lexicographic sort over `date()` and
`date_format(..., "%Y-%m")` output remains chronological because the
strings are fixed-width and most-significant-field first.

### Diagnostics

Existing function diagnostics are reused:

- Unknown temporal function names produce `E1401`.
- Wrong arity produces `E1402`.
- Non-string `date_floor` units and non-string `date_format` patterns
  produce `E1403`.
- Unsupported literal unit or pattern tokens produce `E1406`.

Temporal parse failures are data-level failures and return null, like
`to_number(value)` on unparseable text. They do not produce diagnostics
unless a later release adds strict parsing functions.

## Native Coverage

v0.46.5 prioritizes row-runtime correctness and language surface.
Native lowering can be split in one of two ways:

- Preferred: lower the literal-pattern subset of `date`,
  `datetime`, `year`, `month`, `day`, `date_floor`, and `date_format`
  with Polars temporal/string expressions and mark the covered subset
  `native parity`.
- Acceptable fallback: mark temporal scalar functions
  `row-only by design` in the coverage matrix with a new
  `NativeUnsupportedReason::TemporalFunction` reason, then promote
  them in a later native-coverage release.

The coverage matrix MUST be explicit either way. A pipeline using a
temporal function must not be misreported as native-eligible unless
row-vs-native byte parity is proven for that exact function subset.

## Must

- Add temporal scalar functions to the semantic registry.

  Status: Complete (v0.46.5).

  Extend `SCALAR_FUNCTIONS` in `crates/pdl-semantics/src/registry.rs`
  with `date`, `datetime`, `year`, `month`, `day`, `date_floor`, and
  `date_format`. Arity metadata must match the function contracts.
  Completion and hover pick up the registry entries through the
  existing editor-service path.

- Implement row-runtime temporal evaluation.

  Status: Complete (v0.46.5). The pure parse/floor/format helpers landed
  in `crates/pdl-data/src/temporal.rs` instead of `row_eval.rs` so the
  row runtime and the data facade share one implementation; the
  `row_eval.rs` function arms call into them.

  Add helper parsing and formatting code near the existing scalar
  functions in `crates/pdl-exec/src/runtime/row_eval.rs`. Reuse the
  workspace `chrono` dependency. Keep helpers pure: no wall-clock
  reads, local time-zone lookup, process locale, filesystem metadata,
  or environment-variable access.

- Mirror row-runtime semantics in the data facade.

  Status: Complete (v0.46.5). Native lowering is deferred: the planner
  reports the new `NativeUnsupportedReason::TemporalFunction`
  (`temporal-function`) and the coverage matrix records the temporal
  functions as `row-only by design`.

  Add matching `DataScalarFunction` variants and row evaluation in
  `crates/pdl-data/src/engine.rs` so native fallback and data-layer
  tests agree with `pdl-exec`. If native lowering lands in this
  release, add the Polars expression lowering in the same module and
  prove parity. If native lowering is deferred, make the unsupported
  reason explicit in `crates/pdl-exec/src/runtime/native_planning.rs`
  and the coverage matrix.

- Update `PDL_SPEC.md`.

  Status: Complete (v0.46.5).

  Move `date`, `datetime`, `date_floor`, `year`, `month`, and `day`
  out of the "Recommended future scalar functions" list and into the
  normative scalar-function list. Add `date_format` to that list.
  Document accepted input forms, `Z` handling, null-on-parse-failure,
  locale neutrality, supported format tokens, and the absence of
  wall-clock functions.

- Add Git-log analysis examples and tests.

  Status: Complete (v0.46.5), reshaped by maintainer direction: the
  shipped example is the general-purpose
  `examples/monthly_order_totals.pdl` over `examples/order_events.csv`
  rather than a Git-themed fixture. It emits monthly totals by region
  with `date_format(..., "%Y-%m")` keys, and unit tests in `pdl-data` and
  `pdl-exec` prove that `Z` and `+00:00` parse to the same calendar
  fields.

  Add a small fixture modeled on:

  ```csv
  repo_name,commit_hash,author_name,author_date,committer_name,committer_date
  closed-circuit-io,a1b2c3d,Jane Smith,2024-01-15T10:22:33-05:00,Jane Smith,2024-01-15T10:22:33-05:00
  social-api,e4f5g6h,John Doe,2025-02-17T14:20:59Z,John Doe,2025-02-17T14:21:12Z
  ```

  Add at least one example that emits monthly commit counts by
  `repo_name`, and one test that proves `Z` and `+00:00` parse to the
  same calendar fields.

- Update the coverage matrix in lockstep.

  Status: Complete (v0.46.5). Temporal scalar functions are recorded as
  `row-only by design` with the `temporal-function` reason.

  `docs/PDL_NATIVE_COVERAGE.md` and
  `docs/PDL_NATIVE_COVERAGE.csv` must record temporal scalar
  functions as either `native parity` for the proven subset or
  `row-only by design` with an explicit reason. The CSV remains the
  machine-readable source for native eligibility checks.

- Hold the WASM target graph.

  Status: Complete (v0.46.5). Parsing/formatting uses the `chrono`
  dependency already declared by `pdl-data`, which the wasm build
  consumes with default features off; no Polars, Arrow, Parquet,
  filesystem, locale, or timezone-database dependencies were added.

  The temporal functions must work in `pdl-wasm` without adding
  Polars, Arrow, Parquet, filesystem, locale, or timezone-database
  dependencies to the wasm target graph. `chrono` parsing/formatting
  is acceptable only through dependencies already reachable from the
  wasm-safe crates.

- Update release stamps.

  Status: Complete (v0.46.5). NPM consumer pins stay at the published
  `pdl-wasm@0.43.5` / `pdl-editor@0.43.6`; no `0.46.5` browser packages
  are prepared.

  Workspace `Cargo.toml`, `Cargo.lock`, `docs/PDL_SPEC.md`,
  `editors/vscode/package.json`, `editors/vscode/package-lock.json`,
  and any demo manifests bump to `0.46.5`. NPM consumer pins follow
  the repository's package-version rules; do not point consumers at
  unpublished `pdl-wasm` or `pdl-editor` versions unless this change
  is explicitly preparing those packages for publication.

## Should

- Keep the first implementation string-backed.

  Status: Complete (v0.46.5).

  Avoid introducing date/datetime primitive value classes in this
  patch release. A typed temporal value model would require a larger
  design across CSV/Arrow/Parquet IO, schema inference, comparisons,
  joins, WASM ABI, and editor hovers. The motivating Git-log workflow
  only needs stable string keys and numeric calendar fields.

- Use literal units and patterns in native planning.

  Status: Satisfied by deferral (v0.46.5). Native lowering rejects every
  temporal call with the `temporal-function` reason, so no dynamic
  unit/pattern reaches native planning. The row runtime evaluates dynamic
  string units and patterns.

  `date_floor(value, unit)` and `date_format(value, pattern)` should
  require the unit/pattern argument to be a string literal for native
  eligibility. The row runtime may evaluate dynamic string values, but
  native lowering should stay conservative unless it can preserve
  exact row semantics for dynamic patterns.

- Add focused parity tests before broad examples.

  Status: Complete (v0.46.5).

  Unit tests should cover `Z`, positive and negative offsets,
  fractional seconds, date-only input, invalid input returning null,
  `%Y-%m` formatting, and month/year flooring. Example tests should
  then verify the Git-log monthly-count workflow end to end.

- Document strict temporal parsing as future work.

  Status: Complete (v0.46.5). Listed in the spec's recommended future
  scalar functions.

  A later `parse_datetime_strict(value)` or
  `datetime(value, on_error: "error")`-style surface may be useful for
  evidence-quality validation. v0.46.5 keeps the existing scalar
  convention where unparseable coercions return null.

## Could

- Add `weekday(value)` and `hour(value)`.

  Status: Deferred.

  These are useful for activity heatmaps, but they are not necessary
  for the immediate monthly Git-log comparison. Add them later once
  the core parse/format contract is stable.

- Add `date_add` and `date_diff`.

  Status: Deferred.

  Useful for durations and gaps between commits, but not required for
  grouping by month or extracting calendar fields.

- Add graph output.

  Status: Deferred.

  PDL remains a tabular transformation tool in this release. It emits
  `repo_commit_counts_by_month.csv`; charting stays in matplotlib,
  Excel, pandas, or a downstream visualization tool.

- Add Git-log source integration.

  Status: Deferred.

  PDL does not shell out to `git log` in v0.46.5. Repository name,
  commit hash, author, committer, and ISO-strict timestamps should
  enter through ordinary CSV/Arrow/Parquet inputs.

## Validation Notes

Repository-required Rust checks remain authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Focused temporal checks:

```bash
cargo test -p pdl-exec temporal_scalar_functions
cargo test -p pdl-data temporal_scalar_functions
cargo test -p pdl-editor-services temporal_function_completion
```

Example smoke:

```bash
cargo run -p pdl-cli -- run examples/monthly_order_totals.pdl --stdout-format csv
# Expected: one row per region/month with stable YYYY-MM keys.
```

WASM target graph audit:

```bash
cargo tree -p pdl-wasm --target wasm32-unknown-unknown | grep -E 'polars|arrow|parquet'
# must be empty
```

## Non-Goals

- Do not add current-date, current-time, random, environment, or
  filesystem metadata functions.
- Do not consult the host locale or the host timezone database.
- Do not parse ambiguous locale-specific dates.
- Do not add new primitive temporal value classes.
- Do not change CSV, JSON Lines, Arrow IPC, or Parquet IO bytes.
- Do not add charting or plotting output.
- Do not add a `git log` reader or any command-execution stage.
- Do not silently demote any pipeline that runs natively today.

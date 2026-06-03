# PDL v0.13 Plan

Status: Complete / shipped
Target version: 0.13.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_12_PLAN.md`](V0_12_PLAN.md)

## Purpose

PDL v0.13 is the first stream-interoperability expansion after the v0.12
multi-input release. It promotes the smallest coherent slice of the data-boundary
work already described by the spec: native CLI pipelines can read from stdin,
make deterministic format decisions for streams, and emit Arrow IPC streams to
stdout for Unix-style composition with Algraf and other consumers.

The release thesis is: authors should be able to use the existing table stages
over file or stdin sources, then choose CSV or Arrow IPC stream output without
mixing diagnostics into data stdout. This closes the highest-priority gap between
the current CSV-backed implementation and the interop contract in spec sections
0, 5.6, 5.8, 10.5, 10.8, 14.2, and 16.2.

## Must

- Promote Arrow IPC stream stdout to implemented native CLI output.

  Status: Shipped in 0.13.0. `pdl run file.pdl --stdout-format arrow-stream` and
  `save stdout format "arrow-stream"` write valid, deterministic Arrow IPC
  streams for the resulting table. Human-readable diagnostics and logs continue
  to go to stderr when stdout is data. CSV stdout and CSV file output behavior
  remain unchanged.

- Promote stdin loading to an implemented source boundary.

  Status: Shipped in 0.13.0. `load stdin` and `load -` run in the native CLI when a
  stream format is supplied or sniffed. `--stdin-format <format>` and
  `load stdin format "<format>"` must support at least `csv` and
  `arrow-stream`. Source syntax and CLI conflicts produce `E1217` before
  reading stdin. Missing or unavailable stdin produces `E1806`.

- Implement deterministic stream format resolution and stdin sniffing.

  Status: Shipped in 0.13.0. Runtime format selection follows the spec order:
  explicit format clause, CLI override, extension where applicable, magic-byte
  sniffing, text sniffing, then CSV fallback. The sniffer must preserve all
  consumed bytes before handing the stream to the selected decoder. v0.13
  recognizes Arrow IPC stream and UTF-8 CSV streams; it detects unsupported
  Parquet, Arrow IPC file, and JSON/JSON Lines inputs well enough to report a
  deterministic unsupported-format diagnostic instead of silently treating them
  as CSV.

- Add Arrow IPC stream encoding and decoding behind the `pdl-data` facade.

  Status: Shipped in 0.13.0. Arrow reader and writer details stay inside `pdl-data`
  or lower runtime layers and do not leak into parser, semantics,
  editor-services, LSP, WASM protocol shapes, or source syntax. The initial type
  mapping covers the logical types currently produced by CSV-backed pipelines
  and documents or diagnoses unsupported values deterministically.

- Keep shared registries, editor services, LSP, WASM, and static editor assets
  aligned with the promoted formats.

  Status: Shipped in 0.13.0. `arrow-stream` is no longer described as deferred in
  shared format metadata for native load/save/stdout support. Completion,
  hover, semantic token, TextMate, and syntax-configuration behavior remain
  derived from the shared Rust language facts where applicable. WASM and browser
  execution continue to reject Arrow stream output until browser output is
  promoted, and report that limitation through registered diagnostics
  rather than adding TypeScript-side language logic.

- Add runnable stream interop examples and tests.

  Status: Shipped in 0.13.0. Added small deterministic examples covering CSV stdin,
  Arrow-stream stdout, and an Arrow-stream stdin-to-stdout flow that can be
  validated in CI without requiring Algraf. README docs make the new examples
  discoverable. Tests assert clean stdout bytes,
  stderr diagnostics, sniffing-byte preservation, format-conflict diagnostics,
  and deterministic Arrow IPC output.

- Update the normative spec and release stamps.

  Status: Shipped in 0.13.0. `docs/PDL_SPEC.md` describes the v0.13 behavior,
  removes Arrow IPC stream stdout, stdin loading, and promoted stdin sniffing
  from the "does not yet implement" list, and keeps diagnostic-code usage
  aligned with the catalog. `Cargo.toml`/`Cargo.lock`, CLI version output, VS
  Code package manifests, browser demo package manifests, README docs, and
  user-facing release strings are bumped to `0.13.0`.

## Should

- Support Arrow IPC stream file sinks and sources when they reuse the same
  encoder and decoder safely.

  Status: Shipped in 0.13.0. The release gate was Unix stdin/stdout interop. The
  same runtime path supports `save "out.arrow" format "arrow-stream"` and
  `load "in.arrow" format "arrow-stream"` without widening the implementation
  surface or weakening tests. The spec documents extension versus
  explicit-format behavior.

- Record promoted stream decisions in execution plans and dry-run output.

  Status: Shipped in 0.13.0. Existing planning and dry-run surfaces show stdin,
  stdout, selected formats, sniffed formats, and sink choices deterministically.
  This should remain internal or dry-run behavior unless `pdl plan` is promoted
  in a separate plan item.

## Deferred

- Window expressions remain planned syntax for a later mutation-focused
  release.
- Parquet, Arrow IPC file parity, JSON Lines, and configurable CSV dialects
  remain deferred until promoted by a future plan.
- Schema/plan CLI subcommands, CLI formatting, full LSP code actions, and
  cross-document navigation remain deferred.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls remain deferred.

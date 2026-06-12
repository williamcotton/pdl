# PDL v0.51 Plan

Status: Shipped
Target version: 0.51.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_50_PLAN.md`](V0_50_PLAN.md)

## Purpose

PDL v0.51 is an agent ergonomics release. It adds a safe project-level
initialization surface for LLM coding agents and ships a concise PDL language
reference template that downstream projects can keep at their root.

The template exists to prevent agents from hallucinating SQL, Python, shell, or
unimplemented PDL syntax when asked to create `.pdl` source. It should be useful
for Codex, Claude, Google Antigravity (`agy`), and any future tool that reads
root-level agent instructions.

This release does not change PDL source syntax, runtime semantics, data formats,
native coverage, LSP behavior, or the browser packages.

## Release Thesis

Projects that use PDL should be able to opt into agent guidance without
clobbering existing agent instruction files. The CLI should generate one
authoritative language guide and have tool-specific instruction files point to
it.

## Must

- Add a root language guide template for PDL.

  Status: Implemented.

  The CLI must be able to write `PDL_LANG.md` into a target project directory.
  The file should explain enough PDL syntax, stages, expression rules, CLI
  checks, and common agent pitfalls that an LLM can create valid `.pdl` source
  without assuming another language.

- Add safe agent-file initialization.

  Status: Implemented.

  `pdl init --codex`, `pdl init --claude`, and `pdl init --agy` must generate
  root-level agent references:

  - `--codex` writes or updates `AGENTS.md`.
  - `--agy` writes or updates `AGENTS.md`.
  - `--claude` writes or updates `CLAUDE.md`.

  The command must not overwrite an existing different `PDL_LANG.md`. Existing
  `AGENTS.md` or `CLAUDE.md` files must be appended with a short reference block
  unless they already mention `PDL_LANG.md`.

## Should

- Allow combined targets in one command, such as
  `pdl init --codex --claude --agy`.

  Status: Implemented.

- Carry forward v0.50 benchmark observability fields and keep the native parity
  coverage vocabulary unchanged unless a later spec update explicitly changes
  it.

  Status: Implemented. This release does not touch execution observability.

## Could

- Evaluate generated finite-case native expressions for dynamic `col(value)`.

  Status: Proposed.

- Evaluate native target-position self-join lowering for dynamic offset
  windows.

  Status: Proposed.

- Prototype a row-runtime representation rewrite behind benchmarks.

  Status: Proposed.

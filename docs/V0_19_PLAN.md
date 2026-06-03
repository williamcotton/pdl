# PDL v0.19 Plan

Status: Shipped
Target version: 0.19.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_18_PLAN.md`](V0_18_PLAN.md)

## Purpose

PDL v0.19 is a repository automation release. It adds GitHub Actions for the
Rust test suite and for deploying the browser demo to GitHub Pages.

This release does not add source-language semantics. The CLI, LSP, VS Code
client, WASM runtime, browser demo, and examples continue to share the v0.18
language and runtime behavior.

## Must

- Add GitHub Actions CI.

  Status: Shipped in 0.19.0. `.github/workflows/ci.yml` runs formatting,
  clippy over all workspace targets, and the full Rust workspace test suite.

- Add GitHub Pages deployment for the demo.

  Status: Shipped in 0.19.0. `.github/workflows/demo-pages.yml` builds the Vite
  demo with a repository-aware base path, uploads `demo/dist`, and deploys it to
  GitHub Pages.

- Add a README CI badge.

  Status: Shipped in 0.19.0. The README links the CI badge to the `ci.yml`
  workflow in `williamcotton/pdl`.

- Align spec, README, package metadata, and release stamps.

  Status: Shipped in 0.19.0. The workspace version, lockfiles,
  CLI/version output, manifest/language versions, VS Code package metadata,
  browser demo package metadata, README, and `docs/PDL_SPEC.md` are aligned to
  `0.19.0`.

## Should

- Keep the workflows scoped to existing checked-in surfaces.

  Status: Shipped in 0.19.0. CI exercises the Rust workspace, while the Pages
  workflow builds only the browser demo and the WASM artifact it needs.

## Deferred

- Comment-preserving formatter rewrites.
- Configurable formatter width or style options.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work.

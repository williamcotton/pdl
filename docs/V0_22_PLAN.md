# PDL v0.22 Plan

Status: Shipped
Target version: 0.22.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_21_PLAN.md`](V0_21_PLAN.md)

## Purpose

PDL v0.22 corrects the v0.21 distributable packaging path. The VS Code
extension `.vsix` and standalone browser `pdl.wasm` are public release
downloads, so they belong on GitHub Releases rather than in temporary GitHub
Actions artifacts.

This release keeps the language, runtime, editor, LSP, WASM ABI, browser demo,
and examples stable while moving the packaged outputs to durable release assets
with both versioned filenames and `latest` aliases.

## Must

- Publish distributable editor and browser outputs as GitHub Release assets.

  Status: Shipped in 0.22.0. The CI workflow packages the VS Code `.vsix` and
  standalone browser `pdl.wasm`, verifies that their package versions match the
  Rust workspace WASM crate version, and uploads the files to the GitHub Release
  tagged for the current workspace version.

- Keep latest and versioned download names available.

  Status: Shipped in 0.22.0. The release receives
  `pdl-vscode-<version>.vsix`, `pdl-vscode-latest.vsix`,
  `pdl-wasm-<version>.wasm`, and `pdl-wasm-latest.wasm`.

- Align spec, README, package metadata, and release stamps.

  Status: Shipped in 0.22.0. The workspace version, lockfiles, CLI/version
  output, manifest/language versions, VS Code package metadata, browser demo
  package metadata, README, and `docs/PDL_SPEC.md` are aligned to `0.22.0`.

## Should

- Preserve v0.21 shipped behavior while landing the release-asset correction.

  Status: Shipped in 0.22.0. No language, runtime, editor-service, browser demo,
  or WASM ABI behavior changes are introduced.

## Deferred

- Comment-preserving formatter rewrites.
- Configurable formatter width or style options.
- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work.

# PDL v0.30.1 Plan

Status: Implemented
Target version: 0.30.1
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_30_PLAN.md`](V0_30_PLAN.md)

## Purpose

PDL v0.30.1 is a browser editor package patch. It fixes the published
`pdl-editor` package so Vite-specific Monaco worker and Onigasm asset query
imports are owned by host applications rather than emitted from the package
`dist/` entrypoint.

This patch does not change `.pdl` syntax, execution behavior, editor-service
behavior, `pdl-wasm`, or the browser JSON ABI.

## Scope

### Editor Package Asset Contract

Status: Implemented.

Acceptance criteria:

- `pdl-editor` published `dist/index.mjs` and `dist/index.cjs` do not import
  `monaco-editor` workers with `?worker` or Onigasm WASM with `?url`.
- `setupPdlMonaco` and `<PdlEditor />` accept host-provided
  `createEditorWorker` and `onigasmWasmUrl` setup options.
- Hosts can still opt out of package worker setup with `configureWorker: false`.
- The first-party Vite demo imports the worker and Onigasm WASM asset from app
  source and passes them through `setupOptions`.

## Validation

- `npm run build` from `editors/monaco`
- Inspect generated `dist/` for absence of `?worker` and `?url` imports.

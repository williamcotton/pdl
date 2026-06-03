# PDL VS Code Extension

This extension is a thin VS Code language client for PDL. It starts the Rust
language server with `pdl lsp` by default and does not implement parsing,
analysis, diagnostics, completion, hover, formatting, or code actions in
TypeScript.

Settings:

- `pdl.server.path`: path to the `pdl` executable.
- `pdl.server.args`: arguments used to start the server, defaulting to `["lsp"]`.
- `pdl.trace.server`: LSP tracing level.

Package locally with:

```bash
npm install
npm run package
```

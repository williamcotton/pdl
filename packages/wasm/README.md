# pdl-wasm

Browser runtime loader and structural TypeScript ABI types for PDL `0.27.x`.

During local source-mode development, build or copy `pdl.wasm` into the host
app's public assets and pass that URL explicitly:

```ts
import { loadPdlRuntime } from "pdl-wasm";

const runtime = await loadPdlRuntime({ wasmUrl: "/wasm/pdl.wasm" });
```

For package-surface validation, run `npm run build:wasm` and then
`npm run pack:local`; the generated tarball includes `dist/pdl.wasm`.

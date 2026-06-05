import { defineConfig, type Options } from "tsup";

const common: Options = {
  entry: ["src/index.ts"],
  outDir: "dist",
  platform: "browser",
  target: "es2020",
  outExtension({ format }) {
    return {
      js: format === "esm" ? ".mjs" : ".cjs",
    };
  },
};

export default defineConfig([
  {
    ...common,
    format: ["esm"],
    dts: true,
    clean: true,
    define: {
      __PDL_WASM_MODULE_URL__: "import.meta.url",
    },
  },
  {
    ...common,
    format: ["cjs"],
    dts: true,
    clean: false,
    define: {
      __PDL_WASM_MODULE_URL__: "undefined",
    },
  },
]);

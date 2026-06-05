import { defineConfig } from "tsup";

export default defineConfig({
  entry: ["src/index.tsx"],
  format: ["cjs", "esm"],
  dts: true,
  clean: true,
  outDir: "dist",
  platform: "browser",
  target: "es2020",
  external: [
    /^react(\/.*)?$/,
    /^monaco-editor(\/.*)?$/,
    "monaco-editor-textmate",
    "monaco-textmate",
    /^onigasm(\/.*)?$/,
  ],
  outExtension({ format }) {
    return {
      js: format === "esm" ? ".mjs" : ".cjs",
    };
  },
});

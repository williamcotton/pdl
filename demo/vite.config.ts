import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const base = process.env.VITE_BASE_PATH ?? "/";

export default defineConfig({
  base,
  plugins: [react()],
  resolve: {
    alias: {
      path: new URL("./src/pathBrowser.ts", import.meta.url).pathname,
    },
    dedupe: [
      "monaco-editor",
      "monaco-editor-textmate",
      "monaco-textmate",
      "onigasm",
      "react",
      "react-dom",
    ],
  },
});

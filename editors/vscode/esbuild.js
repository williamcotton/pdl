const esbuild = require("esbuild");

const watch = process.argv.includes("--watch");

const options = {
  entryPoints: ["src/extension.ts"],
  bundle: true,
  outfile: "dist/extension.js",
  external: ["vscode"],
  format: "cjs",
  platform: "node",
  sourcemap: true,
  target: "node20"
};

if (watch) {
  esbuild
    .context(options)
    .then((context) => context.watch())
    .catch((error) => {
      console.error(error);
      process.exit(1);
    });
} else {
  esbuild.build(options).catch((error) => {
    console.error(error);
    process.exit(1);
  });
}

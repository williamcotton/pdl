import { copyFile, stat } from "node:fs/promises";

const distUrl = new URL("../dist/", import.meta.url);
const indexUrl = new URL("index.html", distUrl);
const fallbackUrl = new URL("404.html", distUrl);

await stat(indexUrl);
await copyFile(indexUrl, fallbackUrl);

console.log("Copied dist/index.html to dist/404.html for static route fallback.");

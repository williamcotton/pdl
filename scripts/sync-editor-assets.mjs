import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");

const copies = [
  ["editors/assets/pdl.tmLanguage.json", "editors/vscode/syntaxes/pdl.tmLanguage.json"],
  ["editors/assets/language-configuration.json", "editors/vscode/language-configuration.json"],
  ["editors/assets/pdl.tmLanguage.json", "editors/monaco/assets/pdl.tmLanguage.json"],
  ["editors/assets/language-configuration.json", "editors/monaco/assets/language-configuration.json"],
];

for (const [source, target] of copies) {
  const from = resolve(root, source);
  const to = resolve(root, target);
  mkdirSync(dirname(to), { recursive: true });
  copyFileSync(from, to);
}

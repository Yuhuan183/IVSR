// Fails when a translation drops or renames a {placeholder}. Key parity is
// already enforced by the zh-TW dictionary's TypeScript type.
import { readFileSync } from "node:fs";

const load = (file) => {
  const src = readFileSync(new URL(`../src/lib/i18n/${file}`, import.meta.url), "utf8");
  const entries = {};
  for (const m of src.matchAll(/^\s*"([^"]+)":\s*"((?:[^"\\]|\\.)*)",?$/gm)) entries[m[1]] = m[2];
  return entries;
};
const holes = (s) => [...s.matchAll(/\{([a-z_]+)\}/g)].map((m) => m[1]).sort().join(",");

const en = load("en.ts");
const zh = load("zh-TW.ts");
const problems = [];
for (const [key, text] of Object.entries(en)) {
  if (!(key in zh)) problems.push(`missing zh-TW: ${key}`);
  else if (holes(text) !== holes(zh[key])) problems.push(`placeholders differ: ${key}`);
}
for (const key of Object.keys(zh)) if (!(key in en)) problems.push(`unknown key in zh-TW: ${key}`);
if (Object.keys(en).length < 100) problems.push(`parsed only ${Object.keys(en).length} keys; parser out of date?`);
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log(`i18n ok: ${Object.keys(en).length} keys`);

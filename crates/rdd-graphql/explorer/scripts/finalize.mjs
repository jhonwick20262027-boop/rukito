import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const source = resolve(scriptDir, "../dist/index.html");
const target = resolve(scriptDir, "../../src/explorer.html");

let html = readFileSync(source, "utf8");

const forbidden = [
  { pattern: /<script[^>]*\ssrc\s*=/i, label: "script con src externo" },
  { pattern: /<link[^>]*\srel=["']stylesheet["']/i, label: "hoja de estilo externa" },
  { pattern: /\s(?:src|href)=["'](?:https?:)?\/\//i, label: "URL externa" },
  { pattern: /\beval\s*\(/, label: "eval(" },
  { pattern: /\bnew\s+Function\s*\(/, label: "new Function(" },
];

for (const rule of forbidden) {
  if (rule.pattern.test(html)) {
    console.error(`explorer.html no puede contener ${rule.label} (RF-241, ADR-29).`);
    process.exit(1);
  }
}

if (!html.includes("<style") || !html.includes("<script")) {
  console.error("explorer.html debe llevar CSS y JS inline.");
  process.exit(1);
}

html = html.replace(/\scrossorigin(?:=(?:"[^"]*"|'[^']*'|[^\s>]+))?/g, "");

const banner =
  "<!-- Explorer de rdd · GraphQL. Compilado por crates/rdd-graphql/explorer; no editar a mano (RF-241, ADR-29). -->\n";
const output = banner + html;

writeFileSync(target, output, "utf8");

const kilobytes = (Buffer.byteLength(output, "utf8") / 1024).toFixed(1);
console.log(`explorer.html: ${kilobytes} KB · JS y CSS inline · sin eval ni URLs externas.`);

// Embebe docs/assets/benchmarks.json en docs/benchmark.html.
// El marcador vive entre el <script id="bench-data"> y su cierre; repetir la
// ejecución sustituye el bloque completo, así que el comando es idempotente.
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dataPath = join(root, "docs", "assets", "benchmarks.json");
const pagePath = join(root, "docs", "benchmark.html");

const raw = await readFile(dataPath, "utf8");
const data = JSON.parse(raw);
if (!Array.isArray(data.http) || !Array.isArray(data.ws)) {
  throw new Error("benchmarks.json no tiene las secciones http/ws");
}

const html = await readFile(pagePath, "utf8");
const pattern = /(<script id="bench-data" type="application\/json">)[\s\S]*?(<\/script>)/;
if (!pattern.test(html)) {
  throw new Error("benchmark.html no tiene el bloque #bench-data");
}

const compact = JSON.stringify(data);
const next = html.replace(pattern, `$1${compact}$2`);
await writeFile(pagePath, next, "utf8");

const httpRows = data.http.reduce((total, scenario) => total + scenario.results.length, 0);
const wsRows = data.ws.reduce((total, scenario) => total + scenario.results.length, 0);
console.log(
  `benchmark.html: ${httpRows} filas HTTP y ${wsRows} filas WebSocket embebidas (${compact.length} B).`,
);

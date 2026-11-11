// Genera `docs/especificacion.html` a partir de `specs/RDD-SPEC.md`.
//
// La página incrustaba una copia del spec dentro de un template literal de JS:
// los backticks del Markdown cerraban la cadena y el script entero fallaba, así
// que los tabs no respondían. Aquí el Markdown se convierte a HTML estático en
// el build y la página solo filtra secciones, sin parser en el navegador.
//
// Uso: node scripts/build-spec-page.mjs

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const specPath = join(root, "specs", "RDD-SPEC.md");
const pagePath = join(root, "docs", "especificacion.html");
const mirrorPath = join(root, "docs", "specs", "RDD-SPEC.md");

const spec = readFileSync(specPath, "utf8");

const ESC = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" };
const esc = (text) => text.replace(/[&<>"]/g, (ch) => ESC[ch]);

const plain = (text) =>
  text
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\s+/g, " ")
    .trim();

function inline(text) {
  let out = esc(text);
  const codes = [];
  const stash = (code) => {
    codes.push(code);
    return `\u0000${codes.length - 1}\u0000`;
  };
  // ``code`` primero: el spec lo usa para mostrar backticks dentro del código.
  out = out.replace(/``([\s\S]+?)``/g, (_, code) => stash(code.trim()));
  out = out.replace(/`([^`]+)`/g, (_, code) => stash(code));
  out = out.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  out = out.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<a href="$2">$1</a>');
  return out.replace(/\u0000(\d+)\u0000/g, (_, i) => `<code>${codes[Number(i)]}</code>`);
}

const isTableLine = (line) => line.trimStart().startsWith("|");
const isRule = (line) => /^\s*([-*_])\s*\1\s*\1[\s-*_]*$/.test(line);
const isFence = (line) => /^\s*```/.test(line);

function splitRow(line) {
  const body = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  const cells = [];
  let current = "";
  let inCode = false;
  for (const ch of body) {
    if (ch === "`") inCode = !inCode;
    if (ch === "|" && !inCode) {
      cells.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  cells.push(current.trim());
  return cells;
}

function renderTable(lines) {
  const rows = lines.map(splitRow);
  const head = rows[0];
  const body = rows.slice(2);
  const th = head.map((cell) => `<th>${inline(cell)}</th>`).join("");
  const trs = body
    .map((row) => {
      const cells = [...row];
      while (cells.length < head.length) cells.push("");
      return `<tr>${cells.map((cell) => `<td>${inline(cell)}</td>`).join("")}</tr>`;
    })
    .join("");
  return `<div class="table-wrap"><table><thead><tr>${th}</tr></thead><tbody>${trs}</tbody></table></div>`;
}

function renderList(lines, ordered) {
  const items = [];
  for (const line of lines) {
    const text = line.replace(/^\s*(?:[-*]|\d+\.)\s+/, "");
    const task = text.match(/^\[([ xX])\]\s*(.*)$/);
    if (task) {
      const done = task[1].toLowerCase() === "x";
      items.push(
        `<li class="task${done ? " done" : ""}"><span class="task-box" aria-hidden="true">${done ? "✓" : ""}</span>${inline(task[2])}</li>`,
      );
    } else {
      items.push(`<li>${inline(text)}</li>`);
    }
  }
  const tag = ordered ? "ol" : "ul";
  return `<${tag}>${items.join("")}</${tag}>`;
}

function renderMarkdown(source, startIndex) {
  const lines = source.split(/\r?\n/);
  const html = [];
  let i = 0;
  let slugs = startIndex;

  while (i < lines.length) {
    const line = lines[i];

    if (line.trim() === "") {
      i++;
      continue;
    }

    if (isFence(line)) {
      const lang = plain(line.trim().slice(3)) || "text";
      const body = [];
      i++;
      while (i < lines.length && !isFence(lines[i])) body.push(lines[i++]);
      i++;
      html.push(`<pre data-lang="${esc(lang)}"><code>${esc(body.join("\n"))}</code></pre>`);
      continue;
    }

    const heading = line.match(/^(#{3,5})\s+(.*)$/);
    if (heading) {
      const level = heading[1].length;
      const text = heading[2].trim();
      slugs++;
      html.push(`<h${level} id="h${slugs}">${inline(text)}</h${level}>`);
      i++;
      continue;
    }

    if (isTableLine(line)) {
      const block = [];
      while (i < lines.length && isTableLine(lines[i])) block.push(lines[i++]);
      html.push(renderTable(block));
      continue;
    }

    if (isRule(line)) {
      html.push("<hr>");
      i++;
      continue;
    }

    if (/^\s*[-*]\s+/.test(line)) {
      const block = [];
      while (i < lines.length) {
        const current = lines[i];
        if (/^\s*[-*]\s+/.test(current)) {
          block.push(current);
          i++;
          continue;
        }
        // Continuación de un ítem: el spec envuelve líneas a 80 columnas y una
        // frase puede partir un `code span` en dos líneas.
        if (/^\s+\S/.test(current) && !isFence(current) && !isTableLine(current)) {
          block[block.length - 1] += " " + current.trim();
          i++;
          continue;
        }
        break;
      }
      html.push(renderList(block, false));
      continue;
    }

    if (/^\s*\d+\.\s+/.test(line)) {
      const block = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) block.push(lines[i++]);
      html.push(renderList(block, true));
      continue;
    }

    const paragraph = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !isFence(lines[i]) &&
      !isTableLine(lines[i]) &&
      !isRule(lines[i]) &&
      !/^#{3,5}\s/.test(lines[i]) &&
      !/^\s*[-*]\s+/.test(lines[i]) &&
      !/^\s*\d+\.\s+/.test(lines[i])
    ) {
      paragraph.push(lines[i].trim());
      i++;
    }
    html.push(`<p>${inline(paragraph.join(" "))}</p>`);
  }

  return { html: html.join("\n"), slugs };
}

const titleMatch = spec.match(/^#\s+(.*)$/m);
const title = titleMatch ? plain(titleMatch[1]) : "Especificación técnica";
const meta = (label) => {
  const match = spec.match(new RegExp(`^\\|\\s*${label}\\s*\\|\\s*(.+?)\\s*\\|\\s*$`, "m"));
  return match ? plain(match[1]) : "";
};
const version = meta("Versión del documento");
const date = meta("Fecha");
const status = meta("Estado");

const parts = spec.split(/^##\s+/m);
const intro = parts.shift();
const sections = parts.map((part, index) => {
  const newline = part.indexOf("\n");
  const heading = newline === -1 ? part : part.slice(0, newline);
  const body = newline === -1 ? "" : part.slice(newline + 1);
  return { index, label: plain(heading), body: body.replace(/\s+$/, "") };
});

let slugSeed = 0;
const rendered = sections.map((section) => {
  const { html, slugs } = renderMarkdown(section.body, slugSeed);
  slugSeed = slugs;
  const id = `s${section.index}`;
  return {
    id,
    label: section.label,
    html: `<section class="spec-section" id="${id}" aria-label="${esc(section.label)}">\n<h2>${inline(section.label)}</h2>\n${html}\n</section>`,
  };
});

const tabs = [
  '<button class="spec-tab active" type="button" data-tab="all">Documento completo</button>',
  ...rendered.map(
    (section, i) =>
      `<button class="spec-tab" type="button" data-tab="${section.id}">${esc(section.label)}</button>`,
  ),
].join("\n        ");

const index = rendered.map(
  (section) =>
    `      { title: ${JSON.stringify(section.label)}, section: "Especificación", keywords: ${JSON.stringify(`${title} ${section.label}`)}, url: "especificacion.html#${section.id}" },`,
);

// SEO y GEO: una sola URL canónica, descripción reutilizada en Open Graph y
// datos estructurados que citan versión y fecha reales del documento.
const site = "https://jhonwick20262027-boop.github.io/rukito/";
const canonical = `${site}especificacion.html`;
const pageTitle = "Especificación técnica de rdd: framework REST DDD en Rust";
const description = `Especificación ${version} del framework rdd: requisitos, arquitectura de cuatro capas y criterios de aceptación.`;
const specLd = {
  "@context": "https://schema.org",
  "@graph": [
    {
      "@type": "TechArticle",
      "@id": `${canonical}#article`,
      headline: pageTitle,
      description,
      url: canonical,
      mainEntityOfPage: canonical,
      inLanguage: "es",
      version,
      creativeWorkStatus: status,
      dateModified: date,
      proficiencyLevel: "Expert",
      isPartOf: { "@type": "WebSite", "@id": `${site}#website`, url: site, name: "rdd" },
      author: { "@type": "Organization", name: "rdd", url: site },
      publisher: {
        "@type": "Organization",
        name: "rdd",
        url: site,
        logo: {
          "@type": "ImageObject",
          url: `${site}assets/img/icon-512.png`,
          width: 512,
          height: 512,
        },
      },
      about: [
        { "@type": "Thing", name: "Rust" },
        { "@type": "Thing", name: "Domain-Driven Design" },
        { "@type": "Thing", name: "Software specification" },
      ],
    },
    {
      "@type": "BreadcrumbList",
      itemListElement: [
        { "@type": "ListItem", position: 1, name: "Inicio", item: site },
        { "@type": "ListItem", position: 2, name: "Especificación", item: canonical },
      ],
    },
  ],
};

const page = `<!DOCTYPE html>
<html lang="es">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>${esc(pageTitle)}</title>
  <meta name="description" content="${esc(description)}">
  <meta name="keywords" content="rdd especificación, requisitos, arquitectura, DDD, Rust, framework REST">
  <meta name="author" content="rdd">
  <meta name="robots" content="index, follow, max-snippet:-1, max-image-preview:large, max-video-preview:-1">
  <meta name="theme-color" content="#0d0b08">
  <link rel="canonical" href="${canonical}">
  <link rel="icon" href="assets/img/favicon.svg" type="image/svg+xml">
  <link rel="icon" href="assets/img/icon-192.png" sizes="192x192" type="image/png">
  <link rel="apple-touch-icon" href="assets/img/icon-192.png">
  <link rel="manifest" href="site.webmanifest">
  <link rel="alternate" type="text/markdown" href="specs/RDD-SPEC.md" title="Especificación RDD en Markdown">
  <meta property="og:type" content="article">
  <meta property="og:site_name" content="rdd">
  <meta property="og:locale" content="es_ES">
  <meta property="og:title" content="${esc(pageTitle)}">
  <meta property="og:description" content="${esc(description)}">
  <meta property="og:url" content="${canonical}">
  <meta property="og:image" content="${site}assets/img/og-cover.png">
  <meta property="og:image:type" content="image/png">
  <meta property="og:image:width" content="1200">
  <meta property="og:image:height" content="630">
  <meta property="og:image:alt" content="rdd: framework REST con dominio claro para Rust">
  <meta property="article:modified_time" content="${esc(date)}">
  <meta name="twitter:card" content="summary_large_image">
  <meta name="twitter:title" content="${esc(pageTitle)}">
  <meta name="twitter:description" content="${esc(description)}">
  <meta name="twitter:image" content="${site}assets/img/og-cover.png">
  <meta name="twitter:image:alt" content="rdd: framework REST con dominio claro para Rust">
  <link rel="stylesheet" href="assets/css/main.css">
  <script type="application/ld+json">
${JSON.stringify(specLd, null, 2)}
  </script>
</head>
<body data-nav="spec">
  <div id="site-header-root"></div>

  <script src="assets/js/highlight.js"></script>
  <script src="assets/js/site.js"></script>

  <article class="spec-layout">
    <nav class="spec-tabs" id="specTabs" aria-label="Secciones de la especificación">
        ${tabs}
    </nav>

    <div class="spec-meta">
      <div>
        <span>Documento: <strong>RDD-SPEC.md</strong></span>
        <span>Versión: <strong>${esc(version)}</strong></span>
        <span>Fecha: <strong>${esc(date)}</strong></span>
        <span>Estado: <strong>${esc(status)}</strong></span>
      </div>
    </div>

    <div class="spec-body" id="spec-body">
${rendered.map((section) => section.html).join("\n")}
    </div>

    <nav class="doc-pager" aria-label="Navegación de la documentación">
      <a class="pager-link prev" href="guide/cli.html"><span class="dir">← Anterior</span><span class="title">Referencia de la CLI</span></a>
      <a class="pager-link next" href="index.html"><span class="dir">Siguiente →</span><span class="title">Inicio</span></a>
    </nav>
  </article>

  <script>
    window.RDD = window.RDD || {};
    RDD.searchIndex = [
${index.join("\n")}
    ];
  </script>
  <script>
    document.addEventListener("DOMContentLoaded", function () {
      var tabs = Array.prototype.slice.call(document.querySelectorAll("#specTabs .spec-tab"));
      var sections = Array.prototype.slice.call(document.querySelectorAll(".spec-section"));

      function activate(id, push) {
        var known = id === "all" || sections.some(function (s) { return s.id === id; });
        if (!known) id = "all";
        tabs.forEach(function (t) { t.classList.toggle("active", t.dataset.tab === id); });
        sections.forEach(function (s) { s.classList.toggle("active", id === "all" || s.id === id); });
        if (!push) return;
        try { history.replaceState(null, "", id === "all" ? location.pathname : "#" + id); } catch (e) { /* file:// */ }
      }

      tabs.forEach(function (tab) {
        tab.addEventListener("click", function () {
          activate(tab.dataset.tab, true);
          window.scrollTo({ top: 0, behavior: "smooth" });
        });
      });

      // Enlaces compartidos y atrás/adelante: el hash es el estado de la pestaña.
      window.addEventListener("hashchange", function () {
        activate(location.hash.replace("#", "") || "all", false);
      });

      activate(location.hash.replace("#", "") || "all", false);
    });
  </script>
</body>
</html>
`;

mkdirSync(dirname(mirrorPath), { recursive: true });
writeFileSync(pagePath, page, "utf8");
writeFileSync(mirrorPath, spec, "utf8");

const codeBlocks = (spec.match(/^\s*```/gm) || []).length / 2;
console.log(
  `especificacion.html: ${sections.length} secciones, ${codeBlocks} bloques de código, versión ${version} (${date})`,
);

/* rdd — comportamiento compartido: chrome, código, búsqueda, ambiente. */
(function () {
  const RDD = window.RDD;
  RDD.config = {
    repo: "https://github.com/jhonwick20262027-boop/rukito",
    version: "v0.1.0",
    specVersion: "0.5.0",
  };
  RDD.searchIndex = [
    { title: "Guía rápida", section: "Inicio", keywords: "rdd, rust, api, empezar", url: "guide/inicio-rapido.html" },
    { title: "Arquitectura", section: "Framework", keywords: "rdd, crates, workspace, dependencias", url: "guide/arquitectura.html" },
    { title: "CLI", section: "Referencia", keywords: "rdd, comandos, new, module, run", url: "guide/cli.html" },
    { title: "Especificación", section: "Referencia", keywords: "rdd, spec, requisitos, arquitectura", url: "especificacion.html" },
  ];

  const root = document.body.dataset.root || ".";
  const url = (p) => root + "/" + p;
  RDD.url = url;

  const LOGO = `
    <svg viewBox="0 0 40 40" fill="none" aria-hidden="true">
      <defs><linearGradient id="rdd-lg" x1="0" y1="0" x2="40" y2="40">
        <stop offset="0" stop-color="#ff5a1f"/><stop offset="1" stop-color="#ffb347"/>
      </linearGradient></defs>
      <path d="M20 3 34.5 11.5v17L20 37 5.5 28.5v-17Z" stroke="url(#rdd-lg)" stroke-width="2.6" stroke-linejoin="round"/>
      <path d="M13 14.5h14M13 20h9.5M13 25.5h14" stroke="url(#rdd-lg)" stroke-width="2.6" stroke-linecap="round"/>
    </svg>`;

  const ICONS = {
    search: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.8-3.8"/></svg>',
    github: '<svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.11.79-.25.79-.55v-2.15c-3.2.7-3.87-1.36-3.87-1.36-.52-1.33-1.28-1.69-1.28-1.69-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.19 1.76 1.19 1.03 1.75 2.69 1.25 3.34.95.1-.75.4-1.25.72-1.54-2.55-.29-5.24-1.28-5.24-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.78 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.7 5.38-5.27 5.67.41.36.78 1.06.78 2.14v3.17c0 .3.21.67.8.55A11.51 11.51 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z"/></svg>',
    copy: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>',
    check: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m4.5 12.5 5 5 10-11"/></svg>',
    menu: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h16"/></svg>',
    arrow: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14m-6-6 6 6-6 6"/></svg>',
  };
  RDD.icons = ICONS;
  RDD.logo = LOGO;

  /* ---------- Header ---------- */
  const NAV = [
    { key: "guide", label: "Guía", href: "guide/index.html" },
    { key: "arch", label: "Arquitectura", href: "guide/arquitectura.html" },
    { key: "cli", label: "CLI", href: "guide/cli.html" },
    { key: "spec", label: "Especificación", href: "especificacion.html" },
  ];
  const activeNav = document.body.dataset.nav || "";

  function buildHeader() {
    const mount = document.getElementById("site-header-root");
    if (!mount) return;
    mount.innerHTML = `
      <header class="site-header" id="siteHeader">
        <div class="header-inner">
          <a class="brand" href="${url("index.html")}" aria-label="rdd — inicio">
            ${LOGO}
            <span class="brand-name">rdd<em>.</em></span>
            <span class="brand-version">${RDD.config.version}</span>
          </a>
          <nav class="main-nav" aria-label="Principal">
            ${NAV.map((n) => `<a href="${url(n.href)}" class="${activeNav === n.key ? "active" : ""}">${n.label}</a>`).join("")}
          </nav>
          <div class="header-actions">
            <button class="search-trigger" id="searchTrigger" aria-label="Buscar en la documentación">
              ${ICONS.search}<span>Buscar</span><kbd>Ctrl K</kbd>
            </button>
            <a class="icon-btn" href="${RDD.config.repo}" target="_blank" rel="noopener" aria-label="Repositorio en GitHub">${ICONS.github}</a>
            <a class="btn btn-primary btn-sm" href="${url("guide/inicio-rapido.html")}">Empezar</a>
            <button class="menu-toggle" id="menuToggle" aria-label="Abrir menú" aria-expanded="false">${ICONS.menu}</button>
          </div>
        </div>
      </header>
      <nav class="mobile-nav" id="mobileNav" aria-label="Móvil">
        ${NAV.map((n) => `<a href="${url(n.href)}" class="${activeNav === n.key ? "active" : ""}">${n.label}</a>`).join("")}
        <a href="${RDD.config.repo}" target="_blank" rel="noopener">GitHub</a>
      </nav>`;

    const header = document.getElementById("siteHeader");
    const onScroll = () => header.classList.toggle("scrolled", window.scrollY > 12);
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();

    const toggle = document.getElementById("menuToggle");
    const mobileNav = document.getElementById("mobileNav");
    toggle.addEventListener("click", () => {
      const open = mobileNav.classList.toggle("open");
      toggle.setAttribute("aria-expanded", String(open));
    });
    mobileNav.addEventListener("click", (e) => {
      if (e.target.tagName === "A") mobileNav.classList.remove("open");
    });
  }

  /* ---------- Bloques de código: resaltado + copiado ---------- */
  function copyText(text, btn) {
    const done = () => {
      btn.classList.add("copied");
      const original = btn.innerHTML;
      btn.innerHTML = ICONS.check + (btn.dataset.label ? "<span>copiado</span>" : "");
      setTimeout(() => { btn.classList.remove("copied"); btn.innerHTML = original; }, 1600);
    };
    if (navigator.clipboard && window.isSecureContext !== false) {
      navigator.clipboard.writeText(text).then(done).catch(() => fallbackCopy(text, done));
    } else fallbackCopy(text, done);
  }
  function fallbackCopy(text, done) {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.cssText = "position:fixed;opacity:0";
    document.body.appendChild(ta);
    ta.select();
    try { document.execCommand("copy"); done(); } catch (e) { /* sin portapapeles */ }
    ta.remove();
  }
  RDD.copyText = copyText;

  RDD.enhanceCode = function (scope) {
    // Todo `pre` se mejora: el atributo de lenguaje es opcional porque se
    // deduce del contenido. Antes el selector exigía `data-lang` y los bloques
    // sin atributo quedaban sin resaltar y sin botón de copiar.
    (scope || document).querySelectorAll("pre").forEach((pre) => {
      if (pre.dataset.enhanced) return;
      pre.dataset.enhanced = "1";

      let code = pre.querySelector("code");
      if (!code) {
        code = document.createElement("code");
        code.textContent = pre.textContent;
        pre.textContent = "";
        pre.appendChild(code);
      }

      const fromClass = (code.className.match(/language-([\w-]+)/) || [])[1] || "";
      const declared = pre.dataset.lang || code.dataset.lang || fromClass;
      const raw = code.textContent.replace(/^\n+|\s+$/g, "");
      const lang = declared ? RDD.normalizeLang(declared) : RDD.detectLang(raw);
      code.innerHTML = RDD.highlight(raw, lang);

      const wrap = document.createElement("div");
      wrap.className = "code-block";
      pre.parentNode.insertBefore(wrap, pre);
      wrap.appendChild(pre);

      const label = document.createElement("span");
      label.className = "code-lang";
      label.textContent = lang;
      wrap.appendChild(label);

      const btn = document.createElement("button");
      btn.className = "code-copy";
      btn.dataset.label = "1";
      btn.setAttribute("aria-label", "Copiar código");
      btn.innerHTML = ICONS.copy + "<span>copiar</span>";
      btn.addEventListener("click", () => copyText(raw, btn));
      wrap.appendChild(btn);
    });

    (scope || document).querySelectorAll("[data-copy]").forEach((el) => {
      if (el.dataset.copyBound) return;
      el.dataset.copyBound = "1";
      el.addEventListener("click", () => copyText(el.dataset.copy, el));
    });
  };

  /* ---------- Tabs de ventana de código ---------- */
  function initTabs() {
    document.querySelectorAll(".code-window").forEach((win) => {
      const tabs = win.querySelectorAll(".code-tab");
      const panes = win.querySelectorAll(".code-pane");
      tabs.forEach((tab) => {
        tab.addEventListener("click", () => {
          tabs.forEach((t) => t.classList.toggle("active", t === tab));
          panes.forEach((p) => p.classList.toggle("active", p.dataset.pane === tab.dataset.tab));
        });
      });
    });
  }

  /* ---------- Reveal on scroll ---------- */
  function initReveal() {
    const els = document.querySelectorAll(".reveal");
    if (!("IntersectionObserver" in window)) { els.forEach((el) => el.classList.add("in")); return; }
    const io = new IntersectionObserver((entries) => {
      entries.forEach((en) => {
        if (en.isIntersecting) { en.target.classList.add("in"); io.unobserve(en.target); }
      });
    }, { threshold: 0.12, rootMargin: "0px 0px -40px 0px" });
    els.forEach((el) => io.observe(el));
  }

  /* ---------- Brasas del fondo ---------- */
  function initEmbers() {
    const canvas = document.getElementById("embers");
    if (!canvas || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const ctx = canvas.getContext("2d");
    const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    let w = 0, h = 0, parts = [], raf = 0, visible = true;

    function resize() {
      w = canvas.clientWidth; h = canvas.clientHeight;
      canvas.width = w * dpr; canvas.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    function spawn() {
      const n = Math.min(46, Math.floor(w / 30));
      parts = Array.from({ length: n }, () => ({
        x: Math.random() * w,
        y: Math.random() * h,
        r: 0.7 + Math.random() * 1.9,
        vy: 0.12 + Math.random() * 0.4,
        vx: (Math.random() - 0.5) * 0.16,
        a: 0.12 + Math.random() * 0.4,
        hue: Math.random() < 0.72 ? "255,122,45" : "255,179,71",
        ph: Math.random() * Math.PI * 2,
      }));
    }
    function tick(t) {
      raf = requestAnimationFrame(tick);
      if (!visible) return;
      ctx.clearRect(0, 0, w, h);
      for (const p of parts) {
        p.y -= p.vy; p.x += p.vx + Math.sin(t / 1600 + p.ph) * 0.12;
        if (p.y < -8) { p.y = h + 8; p.x = Math.random() * w; }
        const glow = p.a * (0.65 + 0.35 * Math.sin(t / 900 + p.ph));
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fillStyle = "rgba(" + p.hue + "," + glow.toFixed(3) + ")";
        ctx.fill();
      }
    }
    document.addEventListener("visibilitychange", () => { visible = !document.hidden; });
    window.addEventListener("resize", () => { resize(); spawn(); });
    resize(); spawn();
    raf = requestAnimationFrame(tick);
  }

  /* ---------- Paleta de búsqueda ---------- */
  function buildPalette() {
    const overlay = document.createElement("div");
    overlay.className = "palette-overlay";
    overlay.id = "paletteOverlay";
    overlay.innerHTML = `
      <div class="palette" role="dialog" aria-modal="true" aria-label="Buscar en la documentación">
        <div class="palette-input">
          ${ICONS.search}
          <input id="paletteInput" type="text" placeholder="Buscar en la guía, la referencia y la spec…" autocomplete="off" spellcheck="false"/>
          <kbd>esc</kbd>
        </div>
        <div class="palette-results" id="paletteResults"></div>
        <div class="palette-foot">
          <span><kbd>↑</kbd> <kbd>↓</kbd> navegar</span>
          <span><kbd>↵</kbd> abrir</span>
          <span><kbd>esc</kbd> cerrar</span>
        </div>
      </div>`;
    document.body.appendChild(overlay);

    const input = overlay.querySelector("#paletteInput");
    const results = overlay.querySelector("#paletteResults");
    const index = RDD.searchIndex || [];
    let sel = 0, matches = [];

    function render() {
      const q = input.value.trim().toLowerCase();
      matches = !q ? index.slice(0, 9) : index.filter((e) =>
        (e.title + " " + e.section + " " + (e.keywords || "")).toLowerCase().includes(q)
      ).slice(0, 12);
      sel = 0;
      if (!matches.length) {
        results.innerHTML = '<div class="palette-empty">Sin resultados para «' + RDD.esc(input.value.trim()) + '».<br>Prueba con «middleware», «Repo», «Prisma» o «RDD0301».</div>';
        return;
      }
      results.innerHTML = matches.map((e, i) => `
        <button class="palette-item ${i === sel ? "sel" : ""}" data-i="${i}">
          <span class="pi-section">${RDD.esc(e.section)}</span>
          <span class="pi-title">${RDD.esc(e.title)}</span>
          <span class="pi-crumb">${RDD.esc(e.crumb || "")}</span>
        </button>`).join("");
      results.querySelectorAll(".palette-item").forEach((btn) => {
        btn.addEventListener("click", () => go(parseInt(btn.dataset.i, 10)));
        btn.addEventListener("mousemove", () => {
          sel = parseInt(btn.dataset.i, 10);
          results.querySelectorAll(".palette-item").forEach((b) => b.classList.toggle("sel", b === btn));
        });
      });
    }
    function go(i) {
      const e = matches[i];
      if (!e) return;
      close();
      window.location.href = e.url.charAt(0) === "#"
        ? window.location.pathname + e.url
        : url(e.url);
    }
    function open() {
      overlay.classList.add("open");
      input.value = "";
      render();
      requestAnimationFrame(() => input.focus());
    }
    function close() { overlay.classList.remove("open"); }

    input.addEventListener("input", render);
    input.addEventListener("keydown", (e) => {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        sel = (sel + (e.key === "ArrowDown" ? 1 : -1) + matches.length) % Math.max(matches.length, 1);
        results.querySelectorAll(".palette-item").forEach((b, i) => b.classList.toggle("sel", i === sel));
        const cur = results.querySelector(".palette-item.sel");
        if (cur) cur.scrollIntoView({ block: "nearest" });
      } else if (e.key === "Enter") { e.preventDefault(); go(sel); }
      else if (e.key === "Escape") close();
    });
    overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
    document.addEventListener("keydown", (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        overlay.classList.contains("open") ? close() : open();
      } else if (e.key === "Escape" && overlay.classList.contains("open")) close();
    });
    const trigger = document.getElementById("searchTrigger");
    if (trigger) trigger.addEventListener("click", open);
    RDD.openSearch = open;
  }

  /* ---------- Arranque ---------- */
  document.addEventListener("DOMContentLoaded", () => {
    buildHeader();
    RDD.enhanceCode(document);
    initTabs();
    initReveal();
    initEmbers();
    buildPalette();
    document.querySelectorAll("[data-repo-link]").forEach((a) => { a.href = RDD.config.repo; });
  });
})();
/* Resaltado de sintaxis ligero para rust, bash, toml, json y http. */
window.RDD = window.RDD || {};

(function () {
  const ESC = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" };
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ESC[c]);
  RDD.esc = esc;

  const LANGS = {
    rust: [
      ["\\/\\/[^\\n]*|\\/\\*[\\s\\S]*?\\*\\/", "com"],
      ["#!?\\[[^\\]\\n]*\\]", "attr"],
      ['"(?:\\\\.|[^"\\\\\\n])*"', "str"],
      ["'(?:\\\\.|[^'\\\\\\n])'", "str"],
      ["'[a-zA-Z_][a-zA-Z0-9_]*(?!')", "life"],
      ["[a-zA-Z_][a-zA-Z0-9_]*!", "mac"],
      ["\\b(?:async|await|fn|let|mut|impl|struct|enum|trait|pub|use|mod|crate|self|Self|super|where|for|while|loop|if|else|match|return|move|ref|static|const|type|unsafe|as|in|dyn|break|continue)\\b", "kw"],
      ["\\b(?:true|false)\\b", "num"],
      ["\\b[A-Z][A-Za-z0-9_]*\\b", "type"],
      ["\\b\\d[\\d_]*(?:\\.\\d+)?(?:u8|u16|u32|u64|i8|i16|i32|i64|f32|f64|usize)?\\b", "num"],
      ["[a-zA-Z_][a-zA-Z0-9_]*(?=\\s*\\()", "fn"],
    ],
    bash: [
      ["#[^\\n]*", "com"],
      ['"(?:\\\\.|[^"\\\\])*"|\'[^\'\\n]*\'', "str"],
      ["https?:\\/\\/[^\\s\"']+", "str"],
      ["\\$\\{[^}]*\\}|\\$[A-Za-z_][A-Za-z0-9_]*", "life"],
      ["--?[a-zA-Z][\\w-]*", "flag"],
      ["\\b(?:rdd|cargo|curl|cd|rustup|git|mkdir|export|cat|ls|docker|psql)\\b", "cmd"],
      ["\\b\\d+(?:\\.\\d+)*\\b", "num"],
    ],
    toml: [
      ["#[^\\n]*", "com"],
      ["^\\s*\\[[^\\]\\n]+\\]", "attr"],
      ['"(?:\\\\.|[^"\\\\])*"|\'[^\'\\n]*\'', "str"],
      ["^\\s*[A-Za-z_][\\w.-]*(?=\\s*=)", "key"],
      ["\\b(?:true|false)\\b", "kw"],
      ["\\b\\d[\\d_]*(?:\\.\\d+)?\\b", "num"],
    ],
    json: [
      ['"(?:\\\\.|[^"\\\\])*"(?=\\s*:)', "key"],
      ['"(?:\\\\.|[^"\\\\])*"', "str"],
      ["\\b(?:true|false|null)\\b", "kw"],
      ["-?\\b\\d+(?:\\.\\d+)?(?:[eE][+-]?\\d+)?\\b", "num"],
    ],
    http: [
      ["^(?:GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)\\b", "kw"],
      ["^[A-Za-z][A-Za-z-]*(?=:)", "key"],
      ['"(?:\\\\.|[^"\\\\])*"', "str"],
      ["\\bHTTP\\/[\\d.]+\\b", "attr"],
      ["\\b\\d{3}\\b", "num"],
    ],
  };

  const compiled = {};
  function regexFor(lang) {
    if (!compiled[lang]) {
      const rules = LANGS[lang];
      compiled[lang] = {
        re: new RegExp(rules.map((r) => "(" + r[0] + ")").join("|"), "gm"),
        classes: rules.map((r) => r[1]),
      };
    }
    return compiled[lang];
  }

  RDD.highlight = function (code, lang) {
    if (!LANGS[lang]) return esc(code);
    const { re, classes } = regexFor(lang);
    let out = "", last = 0, m;
    re.lastIndex = 0;
    while ((m = re.exec(code)) !== null) {
      if (m.index > last) out += esc(code.slice(last, m.index));
      let cls = "";
      for (let i = 1; i < m.length; i++) {
        if (m[i] !== undefined) { cls = classes[i - 1]; break; }
      }
      out += '<span class="tok-' + cls + '">' + esc(m[0]) + "</span>";
      last = re.lastIndex;
      if (m[0] === "") re.lastIndex++;
    }
    return out + esc(code.slice(last));
  };
})();
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

const oxideChrome = EditorView.theme({
  "&": {
    backgroundColor: "var(--bg)",
    color: "var(--text)",
  },
  ".cm-content": {
    caretColor: "var(--accent)",
  },
  ".cm-gutters": {
    backgroundColor: "var(--bg-subtle)",
    color: "var(--text-muted)",
    borderRight: "1px solid var(--border-muted)",
  },
  ".cm-activeLineGutter": {
    backgroundColor: "color-mix(in srgb, var(--accent) 8%, transparent)",
    color: "var(--text)",
  },
  ".cm-selectionMatch": {
    backgroundColor: "color-mix(in srgb, var(--accent) 14%, transparent)",
  },
  ".cm-matchingBracket, &.cm-focused .cm-matchingBracket": {
    backgroundColor: "color-mix(in srgb, var(--accent) 18%, transparent)",
    outline: "1px solid var(--border-strong)",
  },
  ".cm-tooltip": {
    backgroundColor: "var(--bg-overlay)",
    border: "1px solid var(--border)",
    color: "var(--text)",
  },
  ".cm-tooltip-autocomplete ul li[aria-selected]": {
    backgroundColor: "var(--accent)",
    color: "#1a0d05",
  },
  ".cm-panels": {
    backgroundColor: "var(--bg-subtle)",
    color: "var(--text)",
  },
});

const oxideHighlight = HighlightStyle.define([
  { tag: t.keyword, color: "var(--tok-key)" },
  { tag: t.typeName, color: "var(--tok-type)" },
  { tag: t.propertyName, color: "var(--tok-fn)" },
  { tag: t.variableName, color: "var(--tok-attr)" },
  { tag: t.meta, color: "var(--tok-attr)" },
  { tag: t.string, color: "var(--tok-string)" },
  { tag: t.number, color: "var(--tok-number)" },
  { tag: [t.bool, t.null, t.atom], color: "var(--tok-bool)" },
  { tag: t.comment, color: "var(--tok-comment)", fontStyle: "italic" },
  { tag: [t.punctuation, t.bracket, t.separator], color: "var(--tok-punct)" },
  { tag: t.invalid, color: "var(--danger)" },
]);

export const oxideEditor = [oxideChrome, syntaxHighlighting(oxideHighlight)];

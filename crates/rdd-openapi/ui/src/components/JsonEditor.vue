<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { basicSetup } from "codemirror";
import { indentLess, indentMore } from "@codemirror/commands";
import { EditorState, Prec } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";

import { oxideEditor } from "../lib/editor-theme";
import { jsonLanguage } from "../lib/json-mode";

interface Props {
  modelValue: string;
  label: string;
}

const props = defineProps<Props>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const host = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

onMounted(() => {
  if (host.value === null) return;
  const state = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      jsonLanguage,
      oxideEditor,
      EditorView.lineWrapping,
      placeholder('{ "title": "…" }'),
      EditorView.contentAttributes.of({
        spellcheck: "false",
        autocapitalize: "off",
        "aria-label": props.label,
      }),
      Prec.high(
        keymap.of([
          { key: "Tab", preventDefault: true, run: indentMore, shift: indentLess },
          { key: "Shift-Tab", preventDefault: true, run: indentLess },
        ]),
      ),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) emit("update:modelValue", update.state.doc.toString());
      }),
    ],
  });
  view = new EditorView({ state, parent: host.value });
});

onBeforeUnmount(() => {
  view?.destroy();
  view = null;
});

watch(
  () => props.modelValue,
  (value) => {
    if (view === null) return;
    if (value === view.state.doc.toString()) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value },
    });
  },
);
</script>

<template>
  <div ref="host" class="h-full" />
</template>

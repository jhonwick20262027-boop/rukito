<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Component } from "vue";
import { basicSetup } from "codemirror";
import { acceptCompletion, autocompletion, completionStatus } from "@codemirror/autocomplete";
import { indentLess, indentMore } from "@codemirror/commands";
import { EditorState, Prec } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { Braces, FileJson, KeyRound, LoaderCircle, Play, Square } from "@lucide/vue";

import { oxideEditor } from "../lib/editor-theme";
import { graphqlLanguage } from "../lib/graphql-mode";
import { makeCompletionSource, schemaHover } from "../lib/schema";
import type { OperationKind, SchemaInfo } from "../lib/types";
import Button from "./ui/Button.vue";

interface Props {
  modelValue: string;
  variables: string;
  headers: string;
  schema: SchemaInfo | null;
  running: boolean;
  streaming: boolean;
  kind: OperationKind;
}

const props = defineProps<Props>();
const emit = defineEmits<{
  "update:modelValue": [value: string];
  "update:variables": [value: string];
  "update:headers": [value: string];
  cursor: [line: number, column: number];
  run: [];
  stop: [];
}>();

type TabId = "operation" | "variables" | "headers";

interface Tab {
  id: TabId;
  label: string;
  icon: Component;
}

const tabs: Tab[] = [
  { id: "operation", label: "Operación", icon: Braces },
  { id: "variables", label: "Variables", icon: FileJson },
  { id: "headers", label: "Cabeceras", icon: KeyRound },
];

const activeTab = ref<TabId>("operation");
const host = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

const runLabel = computed(() => (props.kind === "subscription" ? "Suscribirse" : "Ejecutar"));

function reportCursor(editorView: EditorView): void {
  const head = editorView.state.selection.main.head;
  const line = editorView.state.doc.lineAt(head);
  emit("cursor", line.number, head - line.from + 1);
}

onMounted(() => {
  if (host.value === null) return;
  const state = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      graphqlLanguage,
      oxideEditor,
      EditorView.lineWrapping,
      EditorView.contentAttributes.of({
        spellcheck: "false",
        autocapitalize: "off",
        "aria-label": "Documento GraphQL",
      }),
      placeholder("query {\n  __typename\n}"),
      autocompletion({
        override: [makeCompletionSource(() => props.schema)],
        activateOnTyping: true,
        icons: false,
      }),
      schemaHover(() => props.schema),
      Prec.high(
        keymap.of([
          {
            key: "Tab",
            preventDefault: true,
            run: (target) =>
              completionStatus(target.state) === "active"
                ? acceptCompletion(target)
                : indentMore(target),
            shift: (target) => indentLess(target),
          },
          {
            key: "Mod-Enter",
            preventDefault: true,
            run: () => {
              emit("run");
              return true;
            },
          },
        ]),
      ),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) emit("update:modelValue", update.state.doc.toString());
        if (update.docChanged || update.selectionSet) reportCursor(update.view);
      }),
    ],
  });
  view = new EditorView({ state, parent: host.value });
  reportCursor(view);
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

function insert(text: string): void {
  if (view === null) return;
  view.dispatch(view.state.replaceSelection(text));
  view.focus();
}

defineExpose({ insert, focus: () => view?.focus() });

function updateVariables(event: Event): void {
  const target = event.target;
  if (target instanceof HTMLTextAreaElement) emit("update:variables", target.value);
}

function updateHeaders(event: Event): void {
  const target = event.target;
  if (target instanceof HTMLTextAreaElement) emit("update:headers", target.value);
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex h-9 shrink-0 items-stretch gap-0.5 border-b border-line bg-subtle px-1">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        type="button"
        class="relative flex items-center gap-1.5 px-2.5 text-xs transition-colors"
        :class="activeTab === tab.id ? 'text-fg' : 'text-muted hover:text-fg'"
        @click="activeTab = tab.id"
      >
        <component :is="tab.icon" :size="12" />
        {{ tab.label }}
        <span
          v-if="activeTab === tab.id"
          class="absolute inset-x-1 -bottom-px h-0.5 rounded-full bg-accent"
        />
      </button>

      <div class="ml-auto flex items-center gap-1.5 pr-1">
        <Button
          v-if="props.running && props.streaming"
          variant="danger"
          size="sm"
          title="Cerrar la suscripción"
          @click="emit('stop')"
        >
          <Square :size="12" fill="currentColor" />
          Detener
        </Button>
        <Button
          v-else
          variant="primary"
          size="sm"
          :disabled="props.running"
          :title="runLabel + ' (Ctrl+Enter)'"
          @click="emit('run')"
        >
          <LoaderCircle v-if="props.running" :size="12" class="rdd-spin" />
          <Play v-else :size="12" fill="currentColor" />
          {{ props.running ? "Ejecutando…" : runLabel }}
        </Button>
      </div>
    </div>

    <div v-show="activeTab === 'operation'" class="min-h-0 flex-1">
      <div ref="host" class="h-full" />
    </div>

    <div v-show="activeTab === 'variables'" class="flex min-h-0 flex-1 flex-col bg-canvas">
      <textarea
        :value="props.variables"
        spellcheck="false"
        placeholder='{ "id": "…" }'
        class="min-h-0 flex-1 resize-none bg-transparent p-3 font-mono text-[12.5px] leading-relaxed text-fg outline-none placeholder:text-muted"
        @input="updateVariables"
      />
      <p class="border-t border-line-muted bg-subtle px-3 py-1 text-[10px] text-muted">
        Objeto JSON que se envía como <code class="font-mono">variables</code>.
      </p>
    </div>

    <div v-show="activeTab === 'headers'" class="flex min-h-0 flex-1 flex-col bg-canvas">
      <textarea
        :value="props.headers"
        spellcheck="false"
        placeholder='{ "Authorization": "Bearer <token>" }'
        class="min-h-0 flex-1 resize-none bg-transparent p-3 font-mono text-[12.5px] leading-relaxed text-fg outline-none placeholder:text-muted"
        @input="updateHeaders"
      />
      <p class="border-t border-line-muted bg-subtle px-3 py-1 text-[10px] text-muted">
        Cabeceras HTTP extra; se fusionan con <code class="font-mono">Content-Type</code> y
        <code class="font-mono">Accept</code>.
      </p>
    </div>
  </div>
</template>

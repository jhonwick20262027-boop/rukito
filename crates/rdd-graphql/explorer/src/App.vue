<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Boxes, History } from "@lucide/vue";

import AppHeader from "./components/AppHeader.vue";
import DocsPanel from "./components/DocsPanel.vue";
import HistoryPanel from "./components/HistoryPanel.vue";
import OperationEditor from "./components/OperationEditor.vue";
import ResponsePanel from "./components/ResponsePanel.vue";
import { loadHistory, newHistoryEntry, saveHistory } from "./lib/history";
import { buildSchemaInfo, INTROSPECTION_QUERY } from "./lib/schema";
import { theme, toggleTheme } from "./lib/theme";
import { endpoint, execute, operationKind, parseHeaders, subscribe } from "./lib/transport";
import type {
  HistoryEntry,
  OperationKind,
  RunResult,
  SchemaInfo,
  StreamEvent,
} from "./lib/types";
import { describeProblem, errorMessage, isRecord } from "./lib/utils";

const endpointPath = endpoint();

const schema = ref<SchemaInfo | null>(null);
const schemaState = ref<"loading" | "ready" | "error">("loading");
const schemaError = ref<string | null>(null);

const operation = ref("query {\n  __typename\n}");
const variables = ref("{}");
const headers = ref("{}");

const editor = ref<InstanceType<typeof OperationEditor> | null>(null);

const leftTab = ref<"schema" | "history">("schema");
const leftOpen = ref(false);
const result = ref<RunResult | null>(null);
const events = ref<StreamEvent[]>([]);
const streaming = ref(false);
const running = ref(false);
const requestError = ref<string | null>(null);
const history = ref<HistoryEntry[]>(loadHistory());
const cursor = ref({ line: 1, column: 1 });

let controller: AbortController | null = null;

const kind = computed<OperationKind>(() => operationKind(operation.value));
const schemaCount = computed(() =>
  schema.value === null
    ? null
    : schema.value.types.filter((type) => !type.name.startsWith("__")).length,
);
const schemaLoading = computed(() => schemaState.value === "loading");

function isAbort(error: unknown): boolean {
  return error instanceof DOMException && error.name === "AbortError";
}

function graphqlErrorText(body: unknown): string | null {
  if (!isRecord(body)) return null;
  const errors = body.errors;
  if (!Array.isArray(errors)) return null;
  const messages: string[] = [];
  for (const item of errors) {
    if (isRecord(item) && typeof item.message === "string") messages.push(item.message);
  }
  return messages.length === 0 ? null : messages.join(" · ");
}

async function loadSchema(): Promise<void> {
  schemaState.value = "loading";
  schemaError.value = null;
  const parsed = parseHeaders(headers.value);
  if (!parsed.ok) {
    schemaState.value = "error";
    schemaError.value = parsed.error;
    return;
  }
  try {
    const response = await execute(INTROSPECTION_QUERY, "", parsed.value);
    if (response.status < 200 || response.status >= 300) {
      schemaState.value = "error";
      schemaError.value = describeProblem(response.body, response.status, response.statusText);
      return;
    }
    const info = buildSchemaInfo(response.body);
    if (info === null) {
      schemaState.value = "error";
      schemaError.value = graphqlErrorText(response.body) ?? "La introspección no devolvió `__schema`";
      return;
    }
    schema.value = info;
    schemaState.value = "ready";
    schemaError.value = null;
  } catch (error) {
    schemaState.value = "error";
    schemaError.value = errorMessage(error);
  }
}

function pushHistory(kindValue: OperationKind, status: number | null, durationMs: number): void {
  const entry = newHistoryEntry(
    operation.value,
    variables.value,
    headers.value,
    kindValue,
    status,
    durationMs,
  );
  history.value = [entry, ...history.value].slice(0, 30);
  saveHistory(history.value);
}

async function run(): Promise<void> {
  if (running.value) return;
  requestError.value = null;
  const parsed = parseHeaders(headers.value);
  if (!parsed.ok) {
    requestError.value = parsed.error;
    return;
  }

  const currentKind = kind.value;
  controller = new AbortController();
  running.value = true;
  result.value = null;
  events.value = [];
  const started = performance.now();

  if (currentKind === "subscription") {
    streaming.value = true;
    try {
      await subscribe(
        operation.value,
        variables.value,
        parsed.value,
        {
          onEvent: (event) => {
            events.value.push({
              id: events.value.length + 1,
              event: event.event,
              data: event.data,
              raw: event.raw,
              receivedAt: new Date().toISOString(),
            });
          },
        },
        controller.signal,
      );
    } catch (error) {
      if (!isAbort(error)) requestError.value = errorMessage(error);
    } finally {
      running.value = false;
      streaming.value = false;
      controller = null;
      pushHistory(currentKind, null, performance.now() - started);
    }
    return;
  }

  try {
    const response = await execute(operation.value, variables.value, parsed.value, controller.signal);
    result.value = response;
    pushHistory(currentKind, response.status, response.durationMs);
  } catch (error) {
    if (!isAbort(error)) requestError.value = errorMessage(error);
  } finally {
    running.value = false;
    controller = null;
  }
}

function stop(): void {
  controller?.abort();
  controller = null;
  running.value = false;
  streaming.value = false;
}

function clearResult(): void {
  result.value = null;
  events.value = [];
  requestError.value = null;
}

function insert(text: string): void {
  editor.value?.insert(text);
  leftOpen.value = false;
}

function useHistory(entry: HistoryEntry): void {
  operation.value = entry.query;
  variables.value = entry.variables;
  headers.value = entry.headers;
  clearResult();
  leftOpen.value = false;
}

function clearHistory(): void {
  history.value = [];
  saveHistory(history.value);
}

function onCursor(line: number, column: number): void {
  cursor.value = { line, column };
}

onMounted(() => {
  void loadSchema();
});
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-canvas text-fg">
    <AppHeader
      :endpoint="endpointPath"
      :dark="theme === 'dark'"
      :schema-count="schemaCount"
      :schema-state="schemaState"
      @toggle-theme="toggleTheme"
      @reload-schema="loadSchema"
      @toggle-sidebar="leftOpen = !leftOpen"
    />

    <main
      class="relative grid min-h-0 flex-1 grid-cols-1 md:grid-cols-[290px_minmax(0,0.9fr)_minmax(0,1.1fr)]"
    >
      <button
        v-if="leftOpen"
        type="button"
        aria-label="Cerrar explorador"
        class="absolute inset-0 z-10 bg-black/30 md:hidden"
        @click="leftOpen = false"
      />

      <aside
        class="absolute inset-y-0 left-0 z-20 flex w-[280px] min-h-0 flex-col border-r border-line bg-canvas shadow-xl transition-transform duration-200 md:static md:z-auto md:w-auto md:translate-x-0 md:shadow-none md:transition-none"
        :class="leftOpen ? 'translate-x-0' : '-translate-x-full md:translate-x-0'"
      >
        <div class="flex h-9 shrink-0 items-stretch gap-0.5 border-b border-line bg-subtle px-1">
          <button
            type="button"
            class="relative flex items-center gap-1.5 px-2.5 text-xs transition-colors"
            :class="leftTab === 'schema' ? 'text-fg' : 'text-muted hover:text-fg'"
            @click="leftTab = 'schema'"
          >
            <Boxes :size="12" />
            Explorador
            <span
              v-if="leftTab === 'schema'"
              class="absolute inset-x-1 -bottom-px h-0.5 rounded-full bg-accent"
            />
          </button>
          <button
            type="button"
            class="relative flex items-center gap-1.5 px-2.5 text-xs transition-colors"
            :class="leftTab === 'history' ? 'text-fg' : 'text-muted hover:text-fg'"
            @click="leftTab = 'history'"
          >
            <History :size="12" />
            Historial
            <span
              v-if="history.length > 0"
              class="rounded-full border border-line bg-canvas px-1 font-mono text-[9px] text-muted"
            >
              {{ history.length }}
            </span>
            <span
              v-if="leftTab === 'history'"
              class="absolute inset-x-1 -bottom-px h-0.5 rounded-full bg-accent"
            />
          </button>
        </div>

        <div class="min-h-0 flex-1">
          <DocsPanel
            v-show="leftTab === 'schema'"
            class="h-full"
            :schema="schema"
            :loading="schemaLoading"
            :error="schemaError"
            @insert="insert"
            @reload="loadSchema"
          />
          <HistoryPanel
            v-show="leftTab === 'history'"
            class="h-full"
            :entries="history"
            @use="useHistory"
            @clear="clearHistory"
          />
        </div>
      </aside>

      <section class="flex min-h-0 min-w-0 flex-col border-b border-line md:border-r md:border-b-0">
        <OperationEditor
          ref="editor"
          v-model="operation"
          v-model:variables="variables"
          v-model:headers="headers"
          :schema="schema"
          :running="running"
          :streaming="streaming"
          :kind="kind"
          @cursor="onCursor"
          @run="run"
          @stop="stop"
        />
      </section>

      <section class="flex min-h-0 min-w-0 flex-col">
        <ResponsePanel
          :result="result"
          :events="events"
          :streaming="streaming"
          :error="requestError"
          @clear="clearResult"
        />
      </section>
    </main>

    <footer
      class="flex h-7 shrink-0 items-center gap-3 border-t border-line bg-subtle px-3 text-[10.5px] text-muted"
    >
      <span class="inline-flex items-center gap-1.5">
        <span
          class="size-1.5 rounded-full"
          :class="
            schemaState === 'ready'
              ? 'bg-success'
              : schemaState === 'error'
                ? 'bg-danger'
                : 'bg-attention rdd-pulse'
          "
        />
        {{
          schemaState === "ready"
            ? "Esquema listo"
            : schemaState === "error"
              ? "Sin esquema"
              : "Leyendo esquema"
        }}
      </span>
      <span class="h-3 w-px bg-line" />
      <span class="font-mono">línea {{ cursor.line }}, col {{ cursor.column }}</span>
      <span class="ml-auto hidden sm:inline">Ctrl+Enter ejecutar</span>
      <span class="rounded border border-line bg-canvas px-1.5 py-0.5 font-mono text-[9px]">
        {{ kind }}
      </span>
    </footer>
  </div>
</template>

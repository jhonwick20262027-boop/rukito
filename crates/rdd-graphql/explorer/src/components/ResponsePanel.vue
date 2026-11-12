<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { Braces, Check, CircleAlert, Copy, Eraser, Radio } from "@lucide/vue";

import type { RunResult, StreamEvent } from "../lib/types";
import { formatBytes, formatDuration, isRecord, pretty } from "../lib/utils";
import Button from "./ui/Button.vue";
import JsonNode from "./JsonNode.vue";

interface Props {
  result: RunResult | null;
  events: StreamEvent[];
  streaming: boolean;
  error: string | null;
}

const props = defineProps<Props>();
const emit = defineEmits<{ clear: [] }>();

const scroller = ref<HTMLElement | null>(null);
const copied = ref(false);
let copyTimer: number | undefined;

const statusClass = computed(() => {
  if (props.result === null) return "border-line text-muted";
  if (props.result.status >= 200 && props.result.status < 300) {
    return "border-success/30 bg-success-soft text-success";
  }
  return "border-danger/30 bg-danger-soft text-danger";
});

const graphqlErrors = computed(() => {
  if (props.result === null) return 0;
  if (!isRecord(props.result.body)) return 0;
  const errors = props.result.body.errors;
  return Array.isArray(errors) ? errors.length : 0;
});

const bodyText = computed(() => {
  if (props.result === null) return "";
  if (typeof props.result.body === "string") return props.result.body;
  return pretty(props.result.body);
});

function eventTime(receivedAt: string): string {
  return new Date(receivedAt).toLocaleTimeString();
}

async function copy(): Promise<void> {
  const text =
    props.streaming || props.events.length > 0
      ? props.events.map((event) => event.raw).join("\n\n")
      : bodyText.value;
  if (text === "") return;
  try {
    await navigator.clipboard.writeText(text);
    copied.value = true;
    if (copyTimer !== undefined) window.clearTimeout(copyTimer);
    copyTimer = window.setTimeout(() => {
      copied.value = false;
    }, 1500);
  } catch {
    // Sin permiso de portapapeles, el usuario siempre puede seleccionar el texto.
  }
}

onBeforeUnmount(() => {
  if (copyTimer !== undefined) window.clearTimeout(copyTimer);
});

watch(
  () => props.events.length,
  async () => {
    await nextTick();
    if (scroller.value !== null) scroller.value.scrollTop = scroller.value.scrollHeight;
  },
);
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-canvas">
    <div class="flex h-9 shrink-0 items-center gap-2 border-b border-line bg-subtle px-2.5">
      <span class="text-xs font-medium text-fg">Respuesta</span>

      <span
        v-if="props.result !== null"
        class="rounded border px-1.5 py-0.5 font-mono text-[10px] font-medium"
        :class="statusClass"
      >
        {{ props.result.status }} {{ props.result.statusText }}
      </span>

      <span
        v-if="graphqlErrors > 0"
        class="rounded border border-danger/30 bg-danger-soft px-1.5 py-0.5 font-mono text-[10px] text-danger"
      >
        {{ graphqlErrors }} error{{ graphqlErrors === 1 ? "" : "es" }}
      </span>

      <span
        v-if="props.streaming"
        class="inline-flex items-center gap-1 rounded border border-accent/30 bg-accent-soft px-1.5 py-0.5 text-[10px] text-accent"
      >
        <Radio :size="10" class="rdd-pulse" />
        En vivo · {{ props.events.length }}
      </span>

      <span v-if="props.result !== null && !props.streaming" class="ml-auto text-[10px] text-muted">
        {{ formatDuration(props.result.durationMs) }} · {{ formatBytes(props.result.bytes) }}
      </span>
      <span v-else class="ml-auto" />

      <Button
        variant="ghost"
        size="icon"
        title="Copiar respuesta"
        :disabled="props.result === null && props.events.length === 0"
        @click="copy"
      >
        <Check v-if="copied" :size="13" class="text-success" />
        <Copy v-else :size="13" />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title="Limpiar respuesta"
        :disabled="props.result === null && props.events.length === 0 && props.error === null"
        @click="emit('clear')"
      >
        <Eraser :size="13" />
      </Button>
    </div>

    <div ref="scroller" class="min-h-0 flex-1 overflow-auto">
      <div v-if="props.error !== null" class="m-2 rounded-md border border-danger/30 bg-danger-soft p-2.5">
        <div class="flex items-start gap-2">
          <CircleAlert :size="14" class="mt-0.5 shrink-0 text-danger" />
          <p class="min-w-0 break-words text-[11.5px] text-danger">{{ props.error }}</p>
        </div>
      </div>

      <div v-if="props.streaming || props.events.length > 0" class="divide-y divide-line-muted">
        <div v-for="event in props.events" :key="event.id" class="px-2 py-1.5">
          <div class="mb-1 flex items-center gap-2 px-1">
            <span class="font-mono text-[9px] tracking-wide text-muted uppercase">
              event: {{ event.event }}
            </span>
            <span class="ml-auto font-mono text-[9px] text-muted">{{ eventTime(event.receivedAt) }}</span>
          </div>
          <div class="rounded border border-line-muted bg-subtle/40 px-2 py-1">
            <JsonNode :value="event.data" :expand-level="8" />
          </div>
        </div>
        <div v-if="props.streaming && props.events.length === 0" class="flex items-center gap-2 px-4 py-6 text-[11px] text-muted">
          <Radio :size="13" class="rdd-pulse text-accent" />
          Esperando eventos de la suscripción…
        </div>
      </div>

      <div v-else-if="props.result !== null" class="p-3">
        <JsonNode v-if="typeof props.result.body !== 'string'" :value="props.result.body" :expand-level="8" />
        <pre v-else class="font-mono text-[12px] whitespace-pre-wrap text-fg">{{ props.result.body }}</pre>
      </div>

      <div v-else class="flex h-full flex-col items-center justify-center gap-2 px-8 text-center">
        <div class="grid size-9 place-items-center rounded-full border border-line bg-subtle text-muted">
          <Braces :size="16" />
        </div>
        <p class="text-xs text-muted">Ejecuta una operación para ver la respuesta.</p>
        <p class="text-[10px] text-muted">
          <kbd class="rounded border border-line bg-subtle px-1 py-0.5 font-mono">Ctrl</kbd>
          +
          <kbd class="rounded border border-line bg-subtle px-1 py-0.5 font-mono">Enter</kbd>
          también ejecuta.
        </p>
      </div>
    </div>
  </div>
</template>

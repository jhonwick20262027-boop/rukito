<script setup lang="ts">
import { History, Trash2 } from "@lucide/vue";

import type { HistoryEntry } from "../lib/types";
import { historyLabel } from "../lib/history";

interface Props {
  entries: HistoryEntry[];
}

const props = defineProps<Props>();
const emit = defineEmits<{ use: [entry: HistoryEntry]; clear: [] }>();

function relative(at: string): string {
  const elapsed = Date.now() - new Date(at).getTime();
  const minutes = Math.floor(elapsed / 60_000);
  if (minutes < 1) return "ahora";
  if (minutes < 60) return `hace ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `hace ${hours} h`;
  return `hace ${Math.floor(hours / 24)} d`;
}

function statusLabel(entry: HistoryEntry): string {
  if (entry.status === null) return "SSE";
  return String(entry.status);
}

function statusClass(entry: HistoryEntry): string {
  if (entry.status === null) return "border-accent/30 bg-accent-soft text-accent";
  if (entry.status >= 200 && entry.status < 300) {
    return "border-success/30 bg-success-soft text-success";
  }
  return "border-danger/30 bg-danger-soft text-danger";
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex h-9 shrink-0 items-center justify-between border-b border-line px-3">
      <span class="text-[11px] font-medium text-muted">
        {{ props.entries.length }} operación{{ props.entries.length === 1 ? "" : "es" }}
      </span>
      <button
        v-if="props.entries.length > 0"
        type="button"
        class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-muted hover:bg-subtle hover:text-danger"
        title="Vaciar historial"
        @click="emit('clear')"
      >
        <Trash2 :size="11" />
        Vaciar
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div v-if="props.entries.length === 0" class="flex h-full flex-col items-center justify-center gap-2 px-6 text-center">
        <History :size="20" class="text-muted" />
        <p class="text-[11px] text-muted">
          Las operaciones que ejecutes aparecerán aquí, guardadas en este navegador.
        </p>
      </div>

      <button
        v-for="entry in props.entries"
        :key="entry.id"
        type="button"
        class="flex w-full flex-col gap-1 border-b border-line-muted px-3 py-2 text-left hover:bg-subtle"
        :title="entry.query"
        @click="emit('use', entry)"
      >
        <div class="flex items-center gap-1.5">
          <span
            class="rounded border px-1 py-0.5 font-mono text-[9px] font-medium uppercase"
            :class="statusClass(entry)"
          >
            {{ statusLabel(entry) }}
          </span>
          <span class="truncate font-mono text-[11.5px] text-fg">{{ historyLabel(entry) }}</span>
        </div>
        <div class="flex items-center gap-1.5 text-[10px] text-muted">
          <span class="uppercase">{{ entry.kind }}</span>
          <span>·</span>
          <span>{{ relative(entry.at) }}</span>
          <span class="ml-auto">{{ Math.round(entry.durationMs) }} ms</span>
        </div>
      </button>
    </div>
  </div>
</template>

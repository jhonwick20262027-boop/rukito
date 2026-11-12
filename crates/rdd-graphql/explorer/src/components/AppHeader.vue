<script setup lang="ts">
import { computed } from "vue";
import {
  CircleAlert,
  Database,
  LoaderCircle,
  Moon,
  PanelLeft,
  RefreshCw,
  Sun,
  Waypoints,
} from "@lucide/vue";

import Button from "./ui/Button.vue";

interface Props {
  endpoint: string;
  dark: boolean;
  schemaCount: number | null;
  schemaState: "loading" | "ready" | "error";
}

const props = defineProps<Props>();
const emit = defineEmits<{ toggleTheme: []; reloadSchema: []; toggleSidebar: [] }>();

const schemaLabel = computed(() => {
  if (props.schemaState === "loading") return "Cargando esquema…";
  if (props.schemaState === "error") return "Sin esquema";
  const count = props.schemaCount ?? 0;
  return `${count} ${count === 1 ? "tipo" : "tipos"}`;
});
</script>

<template>
  <header class="flex h-12 shrink-0 items-center gap-3 border-b border-line bg-subtle px-3">
    <Button
      variant="ghost"
      size="icon"
      class="-ml-1 md:hidden"
      title="Mostrar explorador"
      @click="emit('toggleSidebar')"
    >
      <PanelLeft :size="15" />
    </Button>

    <div class="flex items-center gap-2">
      <span
        class="grid size-6 place-items-center rounded-md [background-image:var(--grad-rust)] text-[#1a0d05] shadow-[0_0_18px_rgba(255,90,31,0.28)]"
      >
        <Waypoints :size="14" :stroke-width="2.2" />
      </span>
      <span class="font-mono text-sm font-bold tracking-tight">rdd</span>
      <span class="-ml-1 font-mono text-sm text-accent-strong">graphql</span>
      <span
        class="ml-1 rounded-full border border-accent-strong/35 bg-accent-strong/10 px-2 py-0.5 font-mono text-[10px] font-medium tracking-wide text-accent-strong uppercase"
      >
        Sandbox
      </span>
    </div>

    <span class="h-5 w-px shrink-0 bg-line" />

    <div
      class="flex min-w-0 items-center gap-2 rounded-md border border-line bg-canvas px-2.5 py-1"
      title="Endpoint de este explorer"
    >
      <span
        class="size-1.5 shrink-0 rounded-full"
        :class="props.schemaState === 'error' ? 'bg-danger' : 'bg-success'"
      />
      <span class="truncate font-mono text-xs text-muted">{{ props.endpoint }}</span>
    </div>

    <div class="ml-auto flex shrink-0 items-center gap-1.5">
      <button
        type="button"
        class="flex h-7 items-center gap-1.5 rounded-md border border-line bg-canvas px-2 text-xs text-muted transition-colors hover:bg-inset hover:text-fg"
        title="Recargar el esquema con las cabeceras actuales"
        @click="emit('reloadSchema')"
      >
        <LoaderCircle v-if="props.schemaState === 'loading'" :size="13" class="rdd-spin" />
        <CircleAlert
          v-else-if="props.schemaState === 'error'"
          :size="13"
          class="text-danger"
        />
        <Database v-else :size="13" />
        <span class="hidden sm:inline">{{ schemaLabel }}</span>
        <RefreshCw :size="11" class="opacity-60" />
      </button>

      <Button
        variant="ghost"
        size="icon"
        :title="props.dark ? 'Cambiar a tema claro' : 'Cambiar a tema oscuro'"
        @click="emit('toggleTheme')"
      >
        <Sun v-if="props.dark" :size="15" />
        <Moon v-else :size="15" />
      </Button>
    </div>
  </header>
</template>

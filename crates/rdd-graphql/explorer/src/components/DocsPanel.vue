<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  RefreshCw,
  Search,
  X,
} from "@lucide/vue";

import type { FieldDef, NamedType, SchemaInfo } from "../lib/types";
import { formatType, kindLabel, schemaTypes } from "../lib/schema";

interface Props {
  schema: SchemaInfo | null;
  loading: boolean;
  error: string | null;
}

const props = defineProps<Props>();
const emit = defineEmits<{ insert: [text: string]; reload: [] }>();

const search = ref("");
const selected = ref<string | null>(null);

const selectedType = computed<NamedType | null>(() => {
  if (props.schema === null || selected.value === null) return null;
  return props.schema.types.find((type) => type.name === selected.value) ?? null;
});

const types = computed<NamedType[]>(() =>
  props.schema === null ? [] : schemaTypes(props.schema, false),
);

const filteredTypes = computed(() => {
  const query = search.value.trim().toLowerCase();
  if (query === "") return types.value;
  return types.value.filter(
    (type) =>
      type.name.toLowerCase().includes(query) ||
      (type.description ?? "").toLowerCase().includes(query),
  );
});

interface FieldHit {
  owner: NamedType;
  field: FieldDef;
}

const fieldHits = computed<FieldHit[]>(() => {
  const query = search.value.trim().toLowerCase();
  if (query === "" || props.schema === null) return [];
  const hits: FieldHit[] = [];
  for (const owner of types.value) {
    for (const field of owner.fields ?? []) {
      if (field.name.toLowerCase().includes(query)) hits.push({ owner, field });
      if (hits.length >= 40) return hits;
    }
  }
  return hits;
});

const visibleFields = computed<FieldDef[]>(() => {
  const fields = selectedType.value?.fields ?? [];
  const query = search.value.trim().toLowerCase();
  if (query === "") return fields;
  return fields.filter((field) => field.name.toLowerCase().includes(query));
});

const roots = computed(() => {
  if (props.schema === null) return [];
  return [
    { label: "Query", name: props.schema.queryType },
    { label: "Mutation", name: props.schema.mutationType },
    { label: "Subscription", name: props.schema.subscriptionType },
  ].filter((item): item is { label: string; name: string } => item.name !== null);
});

const KIND_STYLES: Record<string, { dot: string; badge: string }> = {
  OBJECT: { dot: "bg-accent", badge: "border-accent/30 text-accent" },
  INPUT_OBJECT: { dot: "bg-attention", badge: "border-attention/30 text-attention" },
  SCALAR: { dot: "bg-muted", badge: "border-line text-muted" },
  ENUM: { dot: "bg-success", badge: "border-success/30 text-success" },
  INTERFACE: { dot: "bg-tok-bool", badge: "border-tok-bool/30 text-tok-bool" },
  UNION: { dot: "bg-tok-bool", badge: "border-tok-bool/30 text-tok-bool" },
};

function kindStyle(kind: string): { dot: string; badge: string } {
  return KIND_STYLES[kind] ?? { dot: "bg-muted", badge: "border-line text-muted" };
}

function select(name: string): void {
  selected.value = name;
  search.value = "";
}

function insert(text: string): void {
  emit("insert", text);
}

watch(
  () => props.schema,
  (schema) => {
    if (selected.value === null) return;
    const exists = schema?.types.some((type) => type.name === selected.value) ?? false;
    if (!exists) selected.value = null;
  },
);
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex h-9 shrink-0 items-center border-b border-line px-2">
      <div
        class="flex h-7 flex-1 items-center gap-1.5 rounded-md border border-transparent px-1.5 text-muted focus-within:border-line focus-within:bg-canvas"
      >
        <Search :size="13" class="shrink-0" />
        <input
          v-model="search"
          type="search"
          spellcheck="false"
          placeholder="Buscar tipos y campos"
          class="min-w-0 flex-1 bg-transparent text-xs text-fg outline-none placeholder:text-muted"
        />
        <button
          v-if="search !== ''"
          type="button"
          class="grid size-4 place-items-center rounded text-muted hover:bg-subtle hover:text-fg"
          title="Limpiar búsqueda"
          @click="search = ''"
        >
          <X :size="12" />
        </button>
      </div>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div v-if="props.loading" class="space-y-2 p-3">
        <div v-for="index in 7" :key="index" class="h-5 animate-pulse rounded bg-subtle" />
        <p class="pt-2 text-[11px] text-muted">Leyendo `__schema`…</p>
      </div>

      <div v-else-if="props.error !== null" class="p-3">
        <div class="rounded-md border border-danger/30 bg-danger-soft p-2.5">
          <div class="flex items-start gap-2">
            <CircleAlert :size="14" class="mt-0.5 shrink-0 text-danger" />
            <div class="min-w-0">
              <p class="text-xs font-medium text-danger">No se pudo cargar el esquema</p>
              <p class="mt-1 break-words text-[11px] text-muted">{{ props.error }}</p>
              <p class="mt-1.5 text-[11px] text-muted">
                Si el endpoint exige sesión, añade
                <code class="rounded bg-canvas px-1 py-0.5 font-mono text-[10px]">Authorization</code>
                en la pestaña Cabeceras.
              </p>
              <button
                type="button"
                class="mt-2 inline-flex h-6 items-center gap-1 rounded border border-line bg-canvas px-2 text-[11px] text-fg hover:bg-inset"
                @click="emit('reload')"
              >
                <RefreshCw :size="11" />
                Reintentar
              </button>
            </div>
          </div>
        </div>
      </div>

      <div v-else-if="selectedType !== null" class="pb-6">
        <div class="sticky top-0 z-10 flex items-center gap-1 border-b border-line bg-subtle/95 px-2 py-1.5 backdrop-blur">
          <button
            type="button"
            class="grid size-6 shrink-0 place-items-center rounded text-muted hover:bg-inset hover:text-fg"
            title="Volver a la lista de tipos"
            @click="selected = null"
          >
            <ChevronLeft :size="14" />
          </button>
          <span class="truncate font-mono text-xs font-semibold text-fg">{{ selectedType.name }}</span>
          <span
            class="ml-auto shrink-0 rounded-full border px-1.5 py-0.5 text-[10px] font-medium"
            :class="kindStyle(selectedType.kind).badge"
          >
            {{ kindLabel(selectedType.kind) }}
          </span>
        </div>

        <div class="px-3 pt-2.5">
          <p v-if="selectedType.description" class="text-[11px] leading-relaxed text-muted">
            {{ selectedType.description }}
          </p>

          <div v-if="selectedType.interfaces !== null && selectedType.interfaces.length > 0" class="mt-2 flex flex-wrap gap-1">
            <span class="text-[10px] text-muted">Interfaces:</span>
            <button
              v-for="item in selectedType.interfaces"
              :key="item.name"
              type="button"
              class="rounded border border-line px-1 font-mono text-[10px] text-accent hover:bg-subtle"
              @click="select(item.name)"
            >
              {{ item.name }}
            </button>
          </div>

          <div v-if="selectedType.possibleTypes !== null && selectedType.possibleTypes.length > 0" class="mt-2 flex flex-wrap gap-1">
            <span class="text-[10px] text-muted">Posibles tipos:</span>
            <button
              v-for="item in selectedType.possibleTypes"
              :key="item.name"
              type="button"
              class="rounded border border-line px-1 font-mono text-[10px] text-accent hover:bg-subtle"
              @click="select(item.name)"
            >
              {{ item.name }}
            </button>
          </div>

          <div v-if="visibleFields.length > 0" class="mt-3">
            <p class="mb-1 text-[10px] font-semibold tracking-wide text-muted uppercase">Campos</p>
            <button
              v-for="field in visibleFields"
              :key="field.name"
              type="button"
              class="group block w-full rounded px-1.5 py-1 text-left hover:bg-subtle"
              :title="`Insertar ${field.name} en el editor`"
              @click="insert(field.name)"
            >
              <span class="font-mono text-[12px] text-accent group-hover:underline">{{ field.name }}</span>
              <span v-if="field.args.length > 0" class="font-mono text-[11px] text-muted">
                ({{ field.args.map((arg) => `${arg.name}: ${formatType(arg.type)}`).join(", ") }})
              </span>
              <span class="text-[11px] text-muted">: </span>
              <span class="font-mono text-[11px] text-tok-bool">{{ formatType(field.type) }}</span>
              <span v-if="field.description" class="mt-0.5 block text-[11px] leading-snug text-muted">
                {{ field.description }}
              </span>
            </button>
          </div>

          <div v-if="selectedType.inputFields !== null && selectedType.inputFields.length > 0" class="mt-3">
            <p class="mb-1 text-[10px] font-semibold tracking-wide text-muted uppercase">Entrada</p>
            <button
              v-for="field in selectedType.inputFields"
              :key="field.name"
              type="button"
              class="group block w-full rounded px-1.5 py-1 text-left hover:bg-subtle"
              @click="insert(field.name)"
            >
              <span class="font-mono text-[12px] text-accent group-hover:underline">{{ field.name }}</span>
              <span class="text-[11px] text-muted">: </span>
              <span class="font-mono text-[11px] text-tok-bool">{{ formatType(field.type) }}</span>
              <span v-if="field.defaultValue" class="ml-1 font-mono text-[10px] text-muted">= {{ field.defaultValue }}</span>
              <span v-if="field.description" class="mt-0.5 block text-[11px] leading-snug text-muted">
                {{ field.description }}
              </span>
            </button>
          </div>

          <div v-if="selectedType.enumValues !== null && selectedType.enumValues.length > 0" class="mt-3">
            <p class="mb-1 text-[10px] font-semibold tracking-wide text-muted uppercase">Valores</p>
            <button
              v-for="value in selectedType.enumValues"
              :key="value.name"
              type="button"
              class="group block w-full rounded px-1.5 py-1 text-left hover:bg-subtle"
              @click="insert(value.name)"
            >
              <span class="font-mono text-[12px] text-success group-hover:underline">{{ value.name }}</span>
              <span v-if="value.description" class="mt-0.5 block text-[11px] leading-snug text-muted">
                {{ value.description }}
              </span>
            </button>
          </div>

          <p class="mt-4 border-t border-line-muted pt-2 text-[10px] text-muted">
            Clic en un campo para insertarlo en el editor.
          </p>
        </div>
      </div>

      <div v-else-if="props.schema !== null" class="pb-6">
        <div v-if="fieldHits.length > 0" class="border-b border-line-muted pb-2">
          <p class="px-3 pt-2.5 pb-1 text-[10px] font-semibold tracking-wide text-muted uppercase">
            Campos
          </p>
          <button
            v-for="hit in fieldHits"
            :key="`${hit.owner.name}.${hit.field.name}`"
            type="button"
            class="flex w-full items-center gap-1.5 px-3 py-1 text-left hover:bg-subtle"
            @click="select(hit.owner.name)"
          >
            <span class="font-mono text-[11.5px] text-accent">{{ hit.field.name }}</span>
            <span class="truncate font-mono text-[10px] text-muted">{{ hit.owner.name }}</span>
            <ChevronRight :size="11" class="ml-auto shrink-0 text-muted" />
          </button>
        </div>

        <div v-if="roots.length > 0 && search === ''" class="border-b border-line-muted pb-2">
          <p class="px-3 pt-2.5 pb-1 text-[10px] font-semibold tracking-wide text-muted uppercase">
            Raíces
          </p>
          <button
            v-for="root in roots"
            :key="root.label"
            type="button"
            class="flex w-full items-center gap-2 px-3 py-1 text-left hover:bg-subtle"
            @click="select(root.name)"
          >
            <span class="size-1.5 rounded-full bg-accent" />
            <span class="text-[11.5px] text-fg">{{ root.label }}</span>
            <span class="ml-auto font-mono text-[10px] text-muted">{{ root.name }}</span>
            <ChevronRight :size="11" class="text-muted" />
          </button>
        </div>

        <p class="px-3 pt-2.5 pb-1 text-[10px] font-semibold tracking-wide text-muted uppercase">
          Tipos
        </p>
        <button
          v-for="type in filteredTypes"
          :key="type.name"
          type="button"
          class="flex w-full items-center gap-2 px-3 py-1 text-left hover:bg-subtle"
          @click="select(type.name)"
        >
          <span class="size-1.5 shrink-0 rounded-full" :class="kindStyle(type.kind).dot" />
          <span class="truncate font-mono text-[11.5px] text-fg">{{ type.name }}</span>
          <span class="ml-auto shrink-0 text-[10px] text-muted">{{ kindLabel(type.kind) }}</span>
        </button>

        <p v-if="filteredTypes.length === 0 && fieldHits.length === 0" class="px-3 py-6 text-center text-[11px] text-muted">
          Sin resultados para “{{ search }}”.
        </p>
      </div>

      <div v-else class="px-4 py-10 text-center">
        <p class="text-[11px] text-muted">Aún no hay esquema. Usa el botón de recarga de la cabecera.</p>
      </div>
    </div>
  </div>
</template>

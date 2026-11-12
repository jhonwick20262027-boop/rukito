<script setup lang="ts">
import { computed, ref } from "vue";
import { ChevronDown, ChevronRight } from "@lucide/vue";

import { isRecord } from "../lib/utils";

interface Props {
  value: unknown;
  name?: string;
  depth?: number;
  expandLevel?: number;
}

const props = withDefaults(defineProps<Props>(), {
  name: undefined,
  depth: 0,
  expandLevel: 4,
});

const collapsed = ref(props.depth >= props.expandLevel);

const isArrayValue = computed(() => Array.isArray(props.value));
const isContainer = computed(() => isArrayValue.value || isRecord(props.value));

const entries = computed<[string, unknown][]>(() => {
  if (Array.isArray(props.value)) {
    return props.value.map((item, index) => [String(index), item]);
  }
  if (isRecord(props.value)) return Object.entries(props.value);
  return [];
});

const open = computed(() => (isArrayValue.value ? "[" : "{"));
const close = computed(() => (isArrayValue.value ? "]" : "}"));

const display = computed(() => {
  const value = props.value;
  if (typeof value === "string") return JSON.stringify(value);
  if (value === null) return "null";
  return String(value);
});

const primitiveClass = computed(() => {
  if (props.value === null) return "text-tok-null italic";
  if (typeof props.value === "number") return "text-tok-number";
  if (typeof props.value === "boolean") return "text-tok-bool";
  if (typeof props.value === "string") return "text-tok-string";
  return "text-fg";
});
</script>

<template>
  <div>
    <div class="flex items-start gap-1">
      <button
        v-if="isContainer"
        type="button"
        class="mt-[3px] grid size-4 shrink-0 place-items-center rounded text-muted hover:bg-subtle hover:text-fg"
        :title="collapsed ? 'Expandir' : 'Contraer'"
        @click="collapsed = !collapsed"
      >
        <ChevronRight v-if="collapsed" :size="12" />
        <ChevronDown v-else :size="12" />
      </button>
      <span v-else class="w-4 shrink-0" />

      <div class="min-w-0 flex-1">
        <div class="break-all">
          <template v-if="props.name !== undefined">
            <span class="text-tok-key">{{ props.name }}</span>
            <span class="text-tok-punct">: </span>
          </template>
          <template v-if="isContainer">
            <span class="text-tok-punct">{{ open }}</span>
            <template v-if="entries.length === 0">
              <span class="text-tok-punct">{{ close }}</span>
            </template>
            <template v-else-if="collapsed">
              <span class="text-muted"> … {{ entries.length }} </span>
              <span class="text-tok-punct">{{ close }}</span>
            </template>
          </template>
          <span v-else :class="primitiveClass">{{ display }}</span>
        </div>

        <div
          v-if="isContainer && !collapsed && entries.length > 0"
          class="ml-1 border-l border-line-muted pl-2"
        >
          <JsonNode
            v-for="[key, child] in entries"
            :key="key"
            :name="key"
            :value="child"
            :depth="props.depth + 1"
            :expand-level="props.expandLevel"
          />
          <span class="text-tok-punct">{{ close }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

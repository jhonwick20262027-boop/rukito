<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Check, Copy, Moon, Play, Search, ShieldCheck, Sun } from "@lucide/vue";

import {
  operationEntries,
  type OpenApiDocument,
  type OperationEntry,
  type Parameter,
} from "./lib/types";

const props = defineProps<{ api: OpenApiDocument }>();

interface Group {
  tag: string;
  description?: string;
  entries: OperationEntry[];
}

interface ProbeResult {
  status: number;
  ms: number;
  body: string;
  ok: boolean;
}

const entries = computed<OperationEntry[]>(() => operationEntries(props.api));
const selectedKey = ref<string>(entries.value.length > 0 ? entryKey(entries.value[0]) : "");
const search = ref("");
const bearer = ref("");
const bodyText = ref("");
const values = ref<Record<string, string>>({});
const probe = ref<ProbeResult | null>(null);
const probeError = ref<string | null>(null);
const sending = ref(false);
const copied = ref(false);
const storedTheme = readTheme();
const dark = ref(storedTheme !== "light");
document.documentElement.classList.toggle("dark", dark.value);

const selected = computed<OperationEntry | undefined>(() =>
  entries.value.find((entry) => entryKey(entry) === selectedKey.value),
);

const filtered = computed<OperationEntry[]>(() => {
  const needle = search.value.trim().toLowerCase();
  if (needle === "") return entries.value;
  return entries.value.filter((entry) =>
    `${entry.method} ${entry.path} ${entry.operation.summary ?? ""}`.toLowerCase().includes(needle),
  );
});

const groups = computed<Group[]>(() => {
  const byTag = new Map<string, OperationEntry[]>();
  for (const entry of filtered.value) {
    const tag = entry.operation.tags[0] ?? "general";
    const bucket = byTag.get(tag);
    if (bucket) bucket.push(entry);
    else byTag.set(tag, [entry]);
  }
  const order = props.api.tags.map((tag) => tag.name);
  const names = [...byTag.keys()].sort((a, b) => rank(a, order) - rank(b, order));
  return names.map((tag) => ({
    tag,
    description: props.api.tags.find((item) => item.name === tag)?.description,
    entries: byTag.get(tag) ?? [],
  }));
});

const parameters = computed<Parameter[]>(() => selected.value?.operation.parameters ?? []);
const queryParameters = computed<Parameter[]>(() =>
  parameters.value.filter((parameter) => parameter.in === "query"),
);
const hasBody = computed<boolean>(
  () => selected.value?.operation.requestBody !== undefined,
);
const requiresBearer = computed<boolean>(
  () => (selected.value?.operation.security.length ?? 0) > 0,
);
const basePath = computed<string>(() => serverPath(props.api.servers[0]?.url ?? ""));
const serverLabel = computed<string>(() => {
  const server = props.api.servers[0];
  if (!server) return location.origin;
  return server.description ? `${server.description} · ${server.url}` : server.url;
});

watch(
  selectedKey,
  () => {
    values.value = {};
    probe.value = null;
    probeError.value = null;
    bodyText.value = bodyTemplate(selected.value);
  },
  { immediate: true },
);

function entryKey(entry: OperationEntry): string {
  return `${entry.method} ${entry.path}`;
}

function rank(tag: string, order: string[]): number {
  const index = order.indexOf(tag);
  if (index >= 0) return index;
  return tag === "general" ? order.length + 1 : order.length;
}

function readTheme(): string {
  try {
    return localStorage.getItem("rdd-docs-theme") ?? "dark";
  } catch {
    return "dark";
  }
}

function toggleTheme(): void {
  dark.value = !dark.value;
  document.documentElement.classList.toggle("dark", dark.value);
  try {
    localStorage.setItem("rdd-docs-theme", dark.value ? "dark" : "light");
  } catch {
    /* almacenamiento no disponible */
  }
}

function serverPath(url: string): string {
  if (/^https?:\/\//.test(url)) {
    try {
      return new URL(url).pathname.replace(/\/$/, "");
    } catch {
      return "";
    }
  }
  return url.replace(/\/$/, "");
}

function bodyTemplate(entry: OperationEntry | undefined): string {
  const media = entry?.operation.requestBody?.content["application/json"];
  if (media && media.example !== undefined) return pretty(media.example);
  return "{}";
}

function pretty(value: unknown): string {
  if (value === undefined) return "—";
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}

function prettyBody(raw: string): string {
  if (raw.trim() === "") return "(sin cuerpo)";
  try {
    const parsed: unknown = JSON.parse(raw);
    return pretty(parsed);
  } catch {
    return raw;
  }
}

function methodClass(method: string): string {
  switch (method) {
    case "get":
      return "border-success/35 bg-success-soft text-success";
    case "post":
      return "border-accent/40 bg-accent-soft text-accent";
    case "put":
    case "patch":
      return "border-attention/35 bg-attention-soft text-attention";
    case "delete":
      return "border-danger/35 bg-danger-soft text-danger";
    default:
      return "border-line bg-inset text-muted";
  }
}

function buildUrl(entry: OperationEntry): string {
  const path = entry.path.replace(/\{([^}]+)\}/g, (_match: string, name: string) =>
    encodeURIComponent(values.value[name] ?? ""),
  );
  const query = queryParameters.value
    .map((parameter) => {
      const value = (values.value[parameter.name] ?? "").trim();
      return value === ""
        ? ""
        : `${encodeURIComponent(parameter.name)}=${encodeURIComponent(value)}`;
    })
    .filter((item) => item !== "")
    .join("&");
  return `${basePath.value}${path}${query === "" ? "" : `?${query}`}`;
}

const curl = computed<string>(() => {
  const entry = selected.value;
  if (!entry) return "";
  const lines = [`curl -X ${entry.method.toUpperCase()} '${buildUrl(entry)}'`];
  if (hasBody.value) {
    lines.push("-H 'Content-Type: application/json'");
    lines.push(`-d '${bodyText.value.replace(/'/g, "'\\''")}'`);
  }
  if (bearer.value.trim() !== "") lines.push("-H 'Authorization: Bearer $TOKEN'");
  return lines.join(" \\\n  ");
});

async function copyCurl(): Promise<void> {
  try {
    await navigator.clipboard.writeText(curl.value);
    copied.value = true;
    window.setTimeout(() => {
      copied.value = false;
    }, 1500);
  } catch {
    copied.value = false;
  }
}

async function send(): Promise<void> {
  const entry = selected.value;
  if (!entry || sending.value) return;
  sending.value = true;
  probe.value = null;
  probeError.value = null;
  const started = performance.now();
  try {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (hasBody.value) headers["Content-Type"] = "application/json";
    const token = bearer.value.trim();
    if (token !== "") headers.Authorization = `Bearer ${token}`;
    const init: RequestInit = { method: entry.method.toUpperCase(), headers };
    if (hasBody.value) init.body = bodyText.value;
    const response = await fetch(buildUrl(entry), init);
    const raw = await response.text();
    probe.value = {
      status: response.status,
      ms: performance.now() - started,
      body: prettyBody(raw),
      ok: response.ok,
    };
  } catch (error) {
    probeError.value = error instanceof Error ? error.message : String(error);
  } finally {
    sending.value = false;
  }
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-canvas text-fg">
    <header class="flex h-12 shrink-0 items-center gap-3 border-b border-line bg-subtle px-3">
      <div class="flex items-center gap-2">
        <span
          class="grid size-6 place-items-center rounded-md font-mono text-[11px] font-bold text-[#1a0d05] shadow-[0_0_18px_rgba(255,90,31,0.28)] [background-image:var(--grad-rust)]"
        >
          r
        </span>
        <span class="font-display text-sm font-semibold tracking-tight">
          rdd<span class="text-accent">.</span> API
        </span>
        <span
          class="rounded-full border border-accent-strong/35 bg-accent-strong/10 px-2 py-0.5 font-mono text-[10px] font-medium tracking-wide text-accent-strong uppercase"
        >
          {{ api.openapi || "3.1" }}
        </span>
      </div>

      <span
        class="hidden min-w-0 items-center gap-2 rounded-md border border-line bg-canvas px-2.5 py-1 font-mono text-[11px] text-muted lg:flex"
        :title="serverLabel"
      >
        <span class="size-1.5 rounded-full bg-success" />
        <span class="truncate">{{ serverLabel }}</span>
      </span>

      <div class="ml-auto flex items-center gap-2">
        <label
          class="flex min-w-0 items-center gap-2 rounded-md border border-line bg-canvas px-2.5 py-1 md:w-56"
        >
          <Search :size="13" class="shrink-0 text-faint" />
          <input
            v-model="search"
            type="search"
            placeholder="Buscar operación"
            class="w-full bg-transparent text-xs outline-none placeholder:text-faint"
          />
        </label>
        <button
          class="flex h-7 items-center gap-1.5 rounded-md border border-line bg-canvas px-2 text-xs text-muted transition-colors hover:bg-inset hover:text-fg"
          :title="dark ? 'Tema claro' : 'Tema oscuro'"
          @click="toggleTheme"
        >
          <Sun v-if="dark" :size="14" />
          <Moon v-else :size="14" />
        </button>
      </div>
    </header>

    <div class="flex min-h-0 flex-1">
      <aside class="hidden w-72 shrink-0 flex-col border-r border-line bg-subtle md:flex">
        <div class="border-b border-line-muted px-4 py-3">
          <p class="text-[11px] font-medium tracking-wide text-faint uppercase">Operaciones</p>
          <p class="mt-0.5 text-xs text-muted">{{ entries.length }} documentadas</p>
        </div>
        <nav class="min-h-0 flex-1 overflow-y-auto px-2 py-2">
          <div v-for="group in groups" :key="group.tag" class="mb-3">
            <p class="px-2 py-1 font-mono text-[10px] font-semibold tracking-wider text-faint uppercase">
              {{ group.tag }}
            </p>
            <button
              v-for="entry in group.entries"
              :key="entryKey(entry)"
              class="mt-0.5 flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-inset"
              :class="entryKey(entry) === selectedKey ? 'bg-accent-soft' : ''"
              @click="selectedKey = entryKey(entry)"
            >
              <span
                class="mt-0.5 shrink-0 rounded border px-1.5 py-px font-mono text-[9px] font-bold uppercase"
                :class="methodClass(entry.method)"
              >
                {{ entry.method }}
              </span>
              <span class="min-w-0">
                <span class="block truncate font-mono text-[11px] text-fg">{{ entry.path }}</span>
                <span class="block truncate text-[11px] text-muted">{{ entry.operation.summary }}</span>
              </span>
            </button>
          </div>
          <p v-if="groups.length === 0" class="px-2 py-6 text-center text-xs text-muted">
            No hay operaciones que coincidan.
          </p>
        </nav>
      </aside>

      <main class="min-w-0 flex-1 overflow-y-auto">
        <div class="mx-auto flex max-w-5xl flex-col gap-5 px-5 py-6">
          <section class="rounded-xl border border-line bg-overlay p-5 shadow-[var(--shadow)]">
            <div class="flex flex-wrap items-end justify-between gap-3">
              <div>
                <h1 class="font-display text-xl font-semibold tracking-tight">
                  {{ api.info.title }}
                </h1>
                <p class="mt-1 font-mono text-[11px] text-muted">v{{ api.info.version }}</p>
              </div>
              <span
                class="rounded-full border border-line bg-inset px-2.5 py-1 font-mono text-[10px] text-muted"
              >
                {{ api.components.securitySchemes.bearerAuth ? "Bearer JWT" : "Sin seguridad" }}
              </span>
            </div>
            <p v-if="api.info.description" class="mt-3 max-w-3xl text-xs leading-relaxed text-muted">
              {{ api.info.description }}
            </p>
            <div class="mt-4 flex flex-wrap gap-2">
              <span
                v-for="server in api.servers"
                :key="server.url"
                class="rounded-md border border-line bg-canvas px-2 py-1 font-mono text-[10px] text-muted"
              >
                {{ server.url }}
              </span>
            </div>
          </section>

          <label
            v-if="requiresBearer"
            class="flex flex-wrap items-center gap-2 rounded-lg border border-line bg-subtle px-3 py-2"
          >
            <ShieldCheck :size="14" class="text-accent" />
            <span class="text-[11px] font-medium text-muted">Bearer para «Probar»</span>
            <input
              v-model="bearer"
              type="password"
              placeholder="eyJhbGciOi…"
              autocomplete="off"
              class="min-w-0 flex-1 rounded-md border border-line bg-canvas px-2 py-1 font-mono text-[11px] outline-none focus:border-accent"
            />
          </label>

          <section
            v-if="selected"
            class="rounded-xl border border-line bg-overlay shadow-[var(--shadow)]"
          >
            <div class="flex flex-wrap items-center gap-2 border-b border-line-muted px-4 py-3">
              <span
                class="rounded border px-2 py-0.5 font-mono text-[11px] font-bold uppercase"
                :class="methodClass(selected.method)"
              >
                {{ selected.method }}
              </span>
              <span class="min-w-0 break-all font-mono text-xs text-fg">{{ selected.path }}</span>
              <span
                v-if="selected.operation.deprecated"
                class="rounded border border-danger/35 bg-danger-soft px-1.5 py-0.5 text-[10px] text-danger"
              >
                obsoleta
              </span>
              <span v-if="requiresBearer" class="ml-auto flex items-center gap-1 text-[10px] text-muted">
                <ShieldCheck :size="12" class="text-accent" /> requiere Bearer
              </span>
            </div>

            <div class="space-y-4 px-4 py-4">
              <div>
                <h2 class="font-display text-base font-semibold">
                  {{ selected.operation.summary ?? selected.operation.operationId }}
                </h2>
                <p
                  v-if="selected.operation.description"
                  class="mt-1 max-w-3xl text-xs leading-relaxed text-muted"
                >
                  {{ selected.operation.description }}
                </p>
                <p class="mt-2 font-mono text-[10px] text-faint">
                  {{ selected.operation.operationId }}
                </p>
              </div>

              <div v-if="parameters.length > 0">
                <h3 class="mb-2 text-[11px] font-semibold tracking-wide text-muted uppercase">
                  Parámetros
                </h3>
                <div class="overflow-hidden rounded-lg border border-line">
                  <div
                    v-for="parameter in parameters"
                    :key="`${parameter.in}:${parameter.name}`"
                    class="grid grid-cols-[9rem_4rem_1fr] items-start gap-3 border-b border-line-muted px-3 py-2 last:border-b-0"
                  >
                    <div class="min-w-0">
                      <p class="truncate font-mono text-[11px] text-fg">{{ parameter.name }}</p>
                      <p class="text-[10px] text-faint">
                        {{ parameter.in }}{{ parameter.required ? " · obligatorio" : "" }}
                      </p>
                    </div>
                    <p class="truncate font-mono text-[10px] text-muted">
                      {{ pretty(parameter.schema).replace(/\n/g, " ") }}
                    </p>
                    <input
                      v-model="values[parameter.name]"
                      :placeholder="parameter.description ?? parameter.example?.toString() ?? parameter.name"
                      class="rounded-md border border-line bg-canvas px-2 py-1 font-mono text-[11px] outline-none focus:border-accent"
                    />
                  </div>
                </div>
              </div>

              <div v-if="hasBody">
                <h3 class="mb-2 text-[11px] font-semibold tracking-wide text-muted uppercase">
                  Cuerpo · application/json
                </h3>
                <textarea
                  v-model="bodyText"
                  spellcheck="false"
                  class="h-36 w-full resize-y rounded-lg border border-line bg-inset px-3 py-2 font-mono text-[11px] leading-relaxed outline-none focus:border-accent"
                />
              </div>

              <div>
                <h3 class="mb-2 text-[11px] font-semibold tracking-wide text-muted uppercase">
                  Respuestas
                </h3>
                <div class="space-y-1.5">
                  <details
                    v-for="(response, status) in selected.operation.responses"
                    :key="status"
                    class="rounded-lg border border-line bg-canvas px-3 py-2"
                  >
                    <summary class="flex cursor-pointer items-center gap-2 text-xs">
                      <span
                        class="rounded border px-1.5 py-px font-mono text-[10px]"
                        :class="status.startsWith('2') ? 'border-success/35 bg-success-soft text-success' : 'border-danger/35 bg-danger-soft text-danger'"
                      >
                        {{ status }}
                      </span>
                      <span class="text-muted">{{ response.description }}</span>
                    </summary>
                    <pre
                      v-for="(media, mime) in response.content"
                      :key="mime"
                      class="mt-2 overflow-x-auto rounded-md bg-inset p-2 font-mono text-[10px] leading-relaxed text-muted"
                    >{{ pretty(media.schema) }}</pre>
                  </details>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-2 border-t border-line-muted pt-3">
                <button
                  class="flex items-center gap-1.5 rounded-md border border-accent/50 bg-accent-soft px-3 py-1.5 text-xs font-medium text-accent transition-colors hover:bg-accent hover:text-[#1a0d05] disabled:opacity-50"
                  :disabled="sending"
                  @click="send"
                >
                  <Play :size="13" />
                  {{ sending ? "Enviando…" : "Probar" }}
                </button>
                <button
                  class="flex items-center gap-1.5 rounded-md border border-line bg-canvas px-2.5 py-1.5 text-xs text-muted transition-colors hover:bg-inset hover:text-fg"
                  @click="copyCurl"
                >
                  <Check v-if="copied" :size="13" class="text-success" />
                  <Copy v-else :size="13" />
                  {{ copied ? "copiado" : "copiar cURL" }}
                </button>
                <span class="ml-auto font-mono text-[10px] text-faint">{{ basePath }}{{ selected.path }}</span>
              </div>

              <div
                v-if="probe || probeError"
                class="rounded-lg border border-line bg-canvas p-3"
              >
                <div v-if="probe" class="flex items-center gap-2 text-xs">
                  <span
                    class="rounded border px-1.5 py-px font-mono text-[10px]"
                    :class="probe.ok ? 'border-success/35 bg-success-soft text-success' : 'border-danger/35 bg-danger-soft text-danger'"
                  >
                    {{ probe.status }}
                  </span>
                  <span class="text-muted">{{ probe.ms.toFixed(1) }} ms</span>
                </div>
                <p v-if="probeError" class="text-xs text-danger">{{ probeError }}</p>
                <pre
                  v-if="probe"
                  class="mt-2 max-h-80 overflow-auto rounded-md bg-inset p-2 font-mono text-[10px] leading-relaxed text-muted"
                >{{ probe.body }}</pre>
              </div>
            </div>
          </section>

          <p v-else class="rounded-lg border border-line bg-subtle p-6 text-center text-xs text-muted">
            El documento no declara operaciones.
          </p>
        </div>
      </main>
    </div>
  </div>
</template>

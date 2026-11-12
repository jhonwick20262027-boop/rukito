import { createApp } from "vue";

import App from "./App.vue";
import { parseDocument, type OpenApiDocument } from "./lib/types";
import "./style.css";

function fail(mount: HTMLElement, message: string): void {
  mount.innerHTML = "";
  const box = document.createElement("div");
  box.className = "grid h-full place-items-center p-8 text-center";
  const title = document.createElement("h1");
  title.className = "text-lg font-semibold";
  title.textContent = "No se pudo cargar la documentación";
  const detail = document.createElement("p");
  detail.className = "mt-2 max-w-lg font-mono text-xs opacity-70";
  detail.textContent = message;
  box.append(title, detail);
  mount.append(box);
}

function start(): void {
  const mount = document.getElementById("app");
  if (!mount) return;
  const node = document.getElementById("rdd-openapi-document");
  const raw = node?.textContent ?? "{}";
  let api: OpenApiDocument;
  try {
    const parsed: unknown = JSON.parse(raw);
    api = parseDocument(parsed);
  } catch (error) {
    fail(mount, error instanceof Error ? error.message : String(error));
    return;
  }
  createApp(App, { api }).mount(mount);
}

start();

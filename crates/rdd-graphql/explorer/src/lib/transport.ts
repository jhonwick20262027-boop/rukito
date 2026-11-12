import type { HeadersResult, OperationKind, RunResult } from "./types";
import { describeProblem, isRecord, parseJson } from "./utils";

export type VariablesResult =
  | { ok: true; value: Record<string, unknown> | null }
  | { ok: false; error: string };

export interface StreamHandlers {
  onEvent: (event: { event: string; data: unknown; raw: string }) => void;
}

export function endpoint(): string {
  return window.location.pathname;
}

export function parseVariables(text: string): VariablesResult {
  if (text.trim() === "") return { ok: true, value: null };
  const parsed = parseJson(text);
  if (!parsed.ok) return { ok: false, error: `Variables: ${parsed.error}` };
  if (parsed.value === null) return { ok: true, value: null };
  if (!isRecord(parsed.value)) return { ok: false, error: "Las variables deben ser un objeto JSON" };
  return { ok: true, value: parsed.value };
}

export function parseHeaders(text: string): HeadersResult {
  if (text.trim() === "") return { ok: true, value: {} };
  const parsed = parseJson(text);
  if (!parsed.ok) return { ok: false, error: `Cabeceras: ${parsed.error}` };
  if (!isRecord(parsed.value)) return { ok: false, error: "Las cabeceras deben ser un objeto JSON" };
  const entries: [string, string][] = [];
  for (const [key, value] of Object.entries(parsed.value)) {
    if (typeof value !== "string") {
      return { ok: false, error: `La cabecera "${key}" debe ser una cadena` };
    }
    entries.push([key, value]);
  }
  return { ok: true, value: Object.fromEntries(entries) };
}

export function operationKind(query: string): OperationKind {
  const cleaned = query.replace(/#[^\n]*/g, " ");
  const head = cleaned.split("{")[0] ?? "";
  const match = /\b(query|mutation|subscription)\b/.exec(head);
  const word = match?.[1];
  if (word === "mutation" || word === "subscription") return word;
  return "query";
}

function requestBody(query: string, variables: Record<string, unknown> | null): string {
  const body: Record<string, unknown> = { query };
  if (variables !== null) body.variables = variables;
  return JSON.stringify(body);
}

export async function execute(
  query: string,
  variablesText: string,
  headers: Record<string, string>,
  signal?: AbortSignal,
): Promise<RunResult> {
  const variables = parseVariables(variablesText);
  if (!variables.ok) throw new Error(variables.error);

  const started = performance.now();
  const response = await fetch(endpoint(), {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/json",
      ...headers,
    },
    body: requestBody(query, variables.value),
    ...(signal === undefined ? {} : { signal }),
  });
  const text = await response.text();
  const durationMs = performance.now() - started;
  const parsed = parseJson(text);
  return {
    status: response.status,
    statusText: response.statusText,
    durationMs,
    bytes: new TextEncoder().encode(text).length,
    body: parsed.ok ? parsed.value : text,
    text,
  };
}

function handleBlock(
  block: string,
  handlers: StreamHandlers,
): void {
  let event = "message";
  const data: string[] = [];
  for (const line of block.split("\n")) {
    if (line.startsWith(":")) continue;
    if (line.startsWith("event:")) {
      event = line.slice(6).trim();
    } else if (line.startsWith("data:")) {
      data.push(line.slice(5).trimStart());
    }
  }
  if (data.length === 0) return;
  const raw = data.join("\n");
  const parsed = parseJson(raw);
  handlers.onEvent({ event, data: parsed.ok ? parsed.value : raw, raw });
}

export async function subscribe(
  query: string,
  variablesText: string,
  headers: Record<string, string>,
  handlers: StreamHandlers,
  signal: AbortSignal,
): Promise<void> {
  const variables = parseVariables(variablesText);
  if (!variables.ok) throw new Error(variables.error);

  const response = await fetch(endpoint(), {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "text/event-stream",
      ...headers,
    },
    body: requestBody(query, variables.value),
    signal,
  });

  if (!response.ok) {
    const text = await response.text();
    const parsed = parseJson(text);
    throw new Error(
      describeProblem(parsed.ok ? parsed.value : null, response.status, response.statusText),
    );
  }
  if (response.body === null) throw new Error("La respuesta no tiene cuerpo SSE");

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true }).replace(/\r\n?/g, "\n");
      let boundary = buffer.indexOf("\n\n");
      while (boundary !== -1) {
        const block = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        handleBlock(block, handlers);
        boundary = buffer.indexOf("\n\n");
      }
    }
    if (buffer.trim() !== "") handleBlock(buffer, handlers);
  } finally {
    reader.releaseLock();
  }
}

import type { HistoryEntry } from "./types";
import { isRecord, parseJson } from "./utils";

const STORAGE_KEY = "rdd.graphql.history";
const LIMIT = 30;

function isHistoryEntry(value: unknown): value is HistoryEntry {
  if (!isRecord(value)) return false;
  return (
    typeof value.id === "string" &&
    typeof value.query === "string" &&
    typeof value.variables === "string" &&
    typeof value.headers === "string" &&
    typeof value.kind === "string" &&
    typeof value.at === "string" &&
    typeof value.durationMs === "number" &&
    (typeof value.status === "number" || value.status === null)
  );
}

export function loadHistory(): HistoryEntry[] {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (stored === null) return [];
    const parsed = parseJson(stored);
    if (!parsed.ok || !Array.isArray(parsed.value)) return [];
    return parsed.value.filter(isHistoryEntry).slice(0, LIMIT);
  } catch {
    return [];
  }
}

export function saveHistory(entries: HistoryEntry[]): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(entries.slice(0, LIMIT)));
  } catch {
    // Sin almacenamiento local el explorer sigue funcionando.
  }
}

export function newHistoryEntry(
  query: string,
  variables: string,
  headers: string,
  kind: string,
  status: number | null,
  durationMs: number,
): HistoryEntry {
  const stamp = new Date();
  return {
    id: `${stamp.getTime()}-${Math.round(Math.random() * 1_000_000)}`,
    query,
    variables,
    headers,
    kind,
    status,
    at: stamp.toISOString(),
    durationMs,
  };
}

export function historyLabel(entry: HistoryEntry): string {
  const cleaned = entry.query.replace(/#[^\n]*/g, " ").trim();
  const first = cleaned.split("\n").find((line) => line.trim() !== "") ?? "Operación";
  return first.trim().slice(0, 48);
}

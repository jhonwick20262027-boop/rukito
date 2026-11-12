import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

import type { ParseResult } from "./types";

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function stringOrNull(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

export function arrayOf(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

export function parseJson(text: string): ParseResult {
  try {
    return { ok: true, value: JSON.parse(text) };
  } catch (error) {
    const message = error instanceof Error ? error.message : "JSON inválido";
    return { ok: false, error: message };
  }
}

export function pretty(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(1, Math.round(ms))} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

export function describeProblem(body: unknown, status: number, statusText: string): string {
  const title = statusText === "" ? `HTTP ${status}` : `${status} ${statusText}`;
  if (isRecord(body)) {
    const detail = stringOrNull(body.detail);
    const problemTitle = stringOrNull(body.title);
    if (detail !== null && detail !== "") {
      return problemTitle !== null ? `${title} · ${problemTitle}: ${detail}` : `${title} · ${detail}`;
    }
  }
  return title;
}

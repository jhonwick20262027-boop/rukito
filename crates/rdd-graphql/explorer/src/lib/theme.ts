import { ref } from "vue";

export type ThemeMode = "light" | "dark";

const STORAGE_KEY = "rdd.graphql.theme";

function initialTheme(): ThemeMode {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    // Sin almacenamiento local se usa el tema del sitio: oscuro.
  }
  return "dark";
}

export const theme = ref<ThemeMode>(initialTheme());

export function applyTheme(): void {
  document.documentElement.classList.toggle("dark", theme.value === "dark");
  document.documentElement.style.colorScheme = theme.value;
}

export function toggleTheme(): void {
  theme.value = theme.value === "dark" ? "light" : "dark";
  try {
    window.localStorage.setItem(STORAGE_KEY, theme.value);
  } catch {
    // La preferencia es cosmética: si no se puede guardar, no pasa nada.
  }
  applyTheme();
}

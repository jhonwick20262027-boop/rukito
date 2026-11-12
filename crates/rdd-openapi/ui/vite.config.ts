import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// Un solo `api-docs.html` con JS y CSS inline: sin CDN ni `eval` (§10.10, ADR-31).
export default defineConfig({
  base: "./",
  plugins: [vue(), tailwindcss(), viteSingleFile({ removeViteModuleLoader: true })],
  build: {
    target: "es2022",
    cssCodeSplit: false,
    assetsInlineLimit: 100_000_000,
    chunkSizeWarningLimit: 100_000,
  },
});

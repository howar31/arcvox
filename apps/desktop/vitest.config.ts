import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// The svelte() plugin lets store.svelte.ts (runes) compile under vitest.
export default defineConfig({
  plugins: [svelte()],
  test: { include: ["src/**/*.test.ts"] },
});

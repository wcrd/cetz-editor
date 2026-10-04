import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  // Relative asset URLs, so the build works under any path (e.g. GitHub Pages' /cetz-editor/).
  base: "./",
  plugins: [svelte()],
  worker: { format: "es" },
  server: {
    port: Number(process.env.PORT) || 5173,
    // Fixtures live at the repo root, outside the web/ project.
    fs: { allow: [".."] },
  },
});

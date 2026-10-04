import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { statSync } from "node:fs";

/** The compiler's size, for download progress: hosts report the compressed size, if any. */
function wasmBytes(): number {
  try {
    return statSync(new URL("./src/lib/wasm/cetz_worker_bg.wasm", import.meta.url)).size;
  } catch {
    return 0;
  }
}

export default defineConfig({
  define: { __WORKER_WASM_BYTES__: wasmBytes() },
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

import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  // Cargo's output and the local scratchpad hold over 100,000 files; watching
  // them made the dev server crawl them all and react to every cargo write.
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/scratchpad/**"] } },
  // The app only ever runs in current WebView2.
  build: { target: "chrome120" },
});

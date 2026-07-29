import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// The build lands in src/web/dist, where the Rust server embeds it with
// include_str!. Filenames are fixed rather than hashed so the server can serve
// them from three constant routes, and so `cargo build` never needs Node.
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: "../src/web/dist",
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: "app.js",
        chunkFileNames: "app.js",
        assetFileNames: "style.css",
      },
    },
  },
  server: {
    // `npm run dev` serves the UI with hot reload and forwards the API to the
    // engine running under `cargo run --release -- serve`.
    proxy: { "/api": "http://127.0.0.1:8080" },
  },
  test: {
    environment: "jsdom",
    globals: false,
  },
});

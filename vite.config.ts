import { resolve } from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri dev: фиксированный порт, без clearScreen, чтобы не терять логи cargo.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    rollupOptions: {
      // Страницы: оверлей, настройки, история, мастер первого запуска
      input: {
        main: resolve(__dirname, "index.html"),
        settings: resolve(__dirname, "settings.html"),
        history: resolve(__dirname, "history.html"),
        welcome: resolve(__dirname, "welcome.html"),
        menu: resolve(__dirname, "menu.html"),
        menu: resolve(__dirname, "menu.html"),
      },
    },
  },
});

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      $types: resolve(__dirname, "./src/types"),
      "@components": resolve(__dirname, "./src/components"),
      "@pages": resolve(__dirname, "./src/pages"),
      "@contexts": resolve(__dirname, "./src/contexts"),
      "@hooks": resolve(__dirname, "./src/hooks"),
      "@api": resolve(__dirname, "./src/api"),
      "@utils": resolve(__dirname, "./src/utils"),
      "@models": resolve(__dirname, "./src/models"),
      "@icons": resolve(__dirname, "./src/icons"),
    },
  },

  server: {
    port: 1420,
    strictPort: true,
    proxy: {
      "/rpc": "http://127.0.0.1:8080",
      "/login": "http://127.0.0.1:8080",
      "/logout": "http://127.0.0.1:8080",
      "/sounds": "http://127.0.0.1:8080",
      "/ws": { target: "ws://127.0.0.1:8080", ws: true },
    },
  },
  envPrefix: ["VITE_"],
}));

import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// In development the UI runs on Vite and talks to a Parrot started with `--port 4799`.
// Parrot only accepts requests from its own origin, so the proxy rewrites it.
const target = process.env.PARROT_URL ?? "http://127.0.0.1:4799";

export default defineConfig({
  plugins: [react()],
  build: { outDir: "dist", emptyOutDir: true, sourcemap: false, target: "es2022" },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target,
        changeOrigin: true,
        configure: (proxy) => proxy.on("proxyReq", (req) => req.setHeader("origin", target)),
      },
      "/ws": {
        target,
        ws: true,
        changeOrigin: true,
        configure: (proxy) => proxy.on("proxyReqWs", (req) => req.setHeader("origin", target)),
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["src/test-setup.ts"],
  },
});

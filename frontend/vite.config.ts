import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5173,
    proxy: {
      // 同源化 API（开发期；生产由反代/同源部署承担——CORS 免除）
      "/api": {
        target: process.env.VITE_API_PROXY_TARGET ?? "http://localhost:8080",
        changeOrigin: true,
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    // Playwright 真链路 e2e 归 Playwright 跑（需真后端+vite dev），不进 vitest
    exclude: ["**/node_modules/**", "tests/e2e/**"],
  },
});

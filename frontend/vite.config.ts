import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  server: {
    port: 5173,
    proxy: {
      // 同源化 API（开发期；生产由反代/同源部署承担——CORS 免除）。
      // connection: close——e2e 实证 Windows 下 keep-alive 空闲 socket 复用竞态间歇性 ECONNRESET（假 500），
      // 每请求关闭上游连接消除竞态（本机回环开销可忽略；仅 dev 生效）
      "/api": {
        target: process.env.VITE_API_PROXY_TARGET ?? "http://localhost:8080",
        changeOrigin: true,
        headers: { connection: "close" },
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

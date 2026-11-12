import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests/e2e",
  timeout: 60_000,
  use: {
    baseURL: process.env.E2E_BASE_URL ?? "http://localhost:5173",
  },
  // 后端与 vite dev 由外部脚本启动（见 tests/e2e/flow.spec.ts 注释）
});

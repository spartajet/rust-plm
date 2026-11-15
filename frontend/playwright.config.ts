import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests/e2e",
  timeout: 60_000,
  // 本机环境对长命 browser 进程不稳（同 node 环回 RST 家族——台账既有裁定）：
  // 失败自动重试一次（重试时 browser 全新，单跑实证必过）
  retries: 2,
  use: {
    baseURL: process.env.E2E_BASE_URL ?? "http://localhost:5173",
  },
  // 后端与 vite dev 由外部脚本启动（见 tests/e2e/flow.spec.ts 注释）
});

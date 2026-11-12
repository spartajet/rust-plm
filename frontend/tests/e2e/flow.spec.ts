import { expect, test } from "@playwright/test";

/**
 * 真链路 e2e（增量7 Task4）。
 * 前置（脚本外部准备）：
 *   - 后端 plm-server 跑在 http://localhost:18095（测试库，PLM_ADMIN_PASSWORD=e2e-pass-123）
 *   - vite dev 跑在 http://localhost:5173（VITE_API_BASE=http://localhost:18095）
 * 链路：登录 → 上传图纸 → 列表可见 → 详情 → submit → approve(admin) → released
 */

const ADMIN_PW = process.env.E2E_ADMIN_PASSWORD ?? "e2e-pass-123";
// 终审 I5：唯一 obj_no 保证 e2e 可重复执行
const OBJ = `DWG-E2E-FE-${Date.now()}`;

test("full journey: login → upload → list → detail → lifecycle", async ({ page }) => {
  // 1) 登录
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  // 2) 进入图纸页 → 打开上传对话框
  await page.goto("/docs");
  await page.click("text=上传图纸");
  await page.fill('[data-test="upload-objno"]', OBJ);
  await page.fill('[data-test="upload-name"]', "前端E2E支架");
  await page.fill('[data-test="upload-material"]', "Q235");
  await page.setInputFiles('[data-test="upload-file"]', {
    name: "fe.dwg",
    mimeType: "application/octet-stream",
    buffer: Buffer.from("%DWG-FRONTEND-E2E"),
  });
  await page.click('[data-test="upload-submit"]');
  // toast 3s 即逝不作断言；等刷新后的行出现（Review Focus #4）
  await expect(page.getByRole("cell", { name: OBJ })).toBeVisible({ timeout: 15_000 });

  // 4) 详情：属性与状态
  await page.goto(`/docs/${OBJ}`);
  await expect(page.getByText("前端E2E支架")).toBeVisible();
  await expect(page.locator('[data-test="state-tag"]')).toHaveText("工作中");

  // 5) 生命周期：submit → review → approve → released（Review Focus #5）
  await page.click('[data-test="act-submit"]');
  await expect(page.locator('[data-test="state-tag"]')).toHaveText("审核中", { timeout: 10_000 });
  await page.click('[data-test="act-approve"]');
  await expect(page.locator('[data-test="state-tag"]')).toHaveText("已发布", { timeout: 10_000 });

  // 6) released 态：submit 按钮不再显示
  await expect(page.locator('[data-test="act-submit"]')).toHaveCount(0);
});

test("guard: unauthenticated /docs redirects to login", async ({ page }) => {
  // 终审 C1：守卫接线钉——未认证直接访问受保护页必须跳 /login
  await page.goto("/docs");
  await page.waitForURL("**/login", { timeout: 10_000 });
  await expect(page.locator('[data-test="login-btn"]')).toBeVisible();
});

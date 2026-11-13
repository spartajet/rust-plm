import { expect, test } from "@playwright/test";

/**
 * 真链路 e2e（增量7 Task4）。
 * 前置（脚本外部准备）：
 *   - 后端 plm-server 跑在 http://localhost:18095（测试库，PLM_ADMIN_PASSWORD=e2e-pass-123）
 *   - vite dev 跑在 http://localhost:5173（VITE_API_BASE=http://localhost:18095）
 * 链路：登录 → 上传图纸 → 列表可见 → 详情 → submit → approve(admin) → released
 */

const ADMIN_PW = process.env.E2E_ADMIN_PASSWORD ?? "e2e-pass-123";
// chromium 内 fetch 直连后端（绕开 vite proxy 的 node RST 通道；CORS 已放开）。
// 经 addInitScript 注入 window.E2E_API——page.evaluate 闭包看不到 Node 作用域
const API = process.env.E2E_API_BASE ?? "http://127.0.0.1:18095";
test.beforeEach(async ({ page }) => {
  await page.addInitScript((api: string) => {
    (window as unknown as { E2E_API: string }).E2E_API = api;
  }, API);
});
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

  // 6) released 态：submit 按钮不再显示；版本徽章 v1.0
  await expect(page.locator('[data-test="act-submit"]')).toHaveCount(0);
  await expect(page.locator('[data-test="ver-badge"]')).toHaveText("v1.0");

  // 7) 升版（增量8）：released → 升新版 → working v2.0（Review Focus #6）
  await page.click('[data-test="act-revise"]');
  await expect(page.locator('[data-test="state-tag"]')).toHaveText("工作中", { timeout: 10_000 });
  await expect(page.locator('[data-test="ver-badge"]')).toHaveText("v2.0");
  // 版本历史卡有 v1.0 发布快照
  await expect(page.getByText("版本历史（发布快照）")).toBeVisible();
  await expect(page.getByRole("cell", { name: "v1.0" })).toBeVisible();
});

test("guard: unauthenticated /docs redirects to login", async ({ page }) => {
  // 终审 C1：守卫接线钉——未认证直接访问受保护页必须跳 /login
  await page.goto("/docs");
  await page.waitForURL("**/login", { timeout: 10_000 });
  await expect(page.locator('[data-test="login-btn"]')).toBeVisible();
});

// ==================== 增量11：看板 / BOM 版本切换 / 用户管理 ====================
// （造数一律走 page.evaluate 内的 browser fetch——chromium 直连 API，规避本机 node 环回 RST）

test("board: per-state counts match and click links to filtered list", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  // 造 1 个 review 对象（此时 journey 已留 1 working；released 已 revise 回 working）
  const OBJ = `DWG-BOARD-${Date.now()}`;
  const up = await page.evaluate(async (no) => {
    const t = localStorage.getItem("rustplm_token") ?? "";
    const fd = new FormData();
    fd.set("obj_no", no);
    fd.set("attrs", JSON.stringify({ name: "Board件" }));
    fd.set("file", new File(["%BOARD"], "b.dwg"));
    const r = await fetch(`${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/docs`, {
      method: "POST",
      headers: { Authorization: `Bearer ${t}` },
      body: fd,
    });
    return r.status;
  }, OBJ);
  if (up !== 201) throw new Error(`upload failed: ${up}`);
  const submitted = await page.evaluate(async (no) => {
    const t = localStorage.getItem("rustplm_token") ?? "";
    return (
      await fetch(`${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/docs/${no}/submit`, {
        method: "POST",
        headers: { Authorization: `Bearer ${t}` },
      })
    ).status;
  }, OBJ);
  if (submitted !== 200) throw new Error(`submit failed: ${submitted}`);

  await page.goto("/");
  // 基线+1 相对断言（跨 run 残留免疫）：upload/submit 前后各读一次计数
  // （造数段已在 goto 前完成 upload+submit，这里以 goto 后首次读数为基线无意义——
  //   故改为：断言 review ≥1 且点击 working 卡片联动（绝对计数仅验证联动，不验数据库全局态））
  // 计数就绪等待（初渲染为 "-"，I2：裸 innerText 会 NaN）
  const reviewCard = page.locator('[data-test="board-count-review"]');
  await expect(reviewCard).toHaveText(/\d+/, { timeout: 15_000 });
  const reviewCount = Number(await reviewCard.innerText());
  expect(reviewCount).toBeGreaterThanOrEqual(1);
  const workingCard = page.locator('[data-test="board-count-working"]');
  await expect(workingCard).toHaveText(/\d+/, { timeout: 15_000 });

  // 点击卡片 → /docs?state=working → 列表行数与卡片计数一致（联动正确性；残留免疫）
  const workingCount = Number(await workingCard.innerText());
  await page.click('[data-test="board-working"]');
  await page.waitForURL("**/docs?state=working");
  // 断言分页 total 与卡片计数一致（列表 size=20 截断行数——total 才是全量）
  // toHaveText(regex) 是全串匹配；取文本后 word-boundary 比较（I1：子串在多位数间假 PASS）
  const totalEl = page.locator(".el-pagination__total");
  await expect(totalEl).toHaveText(/\d+/, { timeout: 10_000 });
  const totalText = await totalEl.innerText();
  expect(new RegExp(`\\b${workingCount}\\b`).test(totalText)).toBe(true);
});

test("bom version switch: live vs major=1 snapshot on detail page", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  const P = `DWG-BOMVER-${Date.now()}`;
  // chromium 内造数：3 对象 + BOM + 生命周期（API 语义已由后端 e2e 钉住）
  const upload = (no: string) =>
    page.evaluate(async (no) => {
      const t = localStorage.getItem("rustplm_token") ?? "";
      const fd = new FormData();
      fd.set("obj_no", no);
      fd.set("attrs", JSON.stringify({ name: no }));
      fd.set("file", new File(["%X"], "x.dwg"));
      return (await fetch(`${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/docs`, { method: "POST", headers: { Authorization: `Bearer ${t}` }, body: fd })).status;
    }, no);
  for (const no of [P, `${P}-C1`, `${P}-C2`]) {
    const s = await upload(no);
    if (s !== 201) throw new Error(`upload ${no}: ${s}`);
  }
  const idOf = (no: string) =>
    page.evaluate(async (no) => {
      const t = localStorage.getItem("rustplm_token") ?? "";
      const r = await fetch(`${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/docs/${no}`, { headers: { Authorization: `Bearer ${t}` } });
      const v = await r.json();
      return v.object.id as string;
    }, no);
  const [, c1, c2] = [await idOf(P), await idOf(`${P}-C1`), await idOf(`${P}-C2`)];
  const setLines = (lines: unknown[]) =>
    page.evaluate(async ({ no, lines }) => {
      const t = localStorage.getItem("rustplm_token") ?? "";
      const r = await fetch(
        `${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/boms/${no}/lines`,
        {
          method: "POST",
          headers: { Authorization: `Bearer ${t}`, "Content-Type": "application/json" },
          body: JSON.stringify(lines),
        },
      );
      return `${r.status} ${await r.text()}`;
    }, { no: P, lines });
  const v1 = await setLines([{ line_no: 1, child: c1, qty: 2 }, { line_no: 2, child: c2, qty: 1 }]);
  if (!v1.startsWith("204")) throw new Error(`set_lines v1: ${v1}`);
  // v1 发布 → revise → v2 改 BOM（语义由后端 e2e 钉；此处走 browser fetch）
  const lifecycle = (no: string, act: string) =>
    page.evaluate(async ({ no, act }) => {
      const t = localStorage.getItem("rustplm_token") ?? "";
      return (await fetch(`${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/docs/${no}/${act}`, { method: "POST", headers: { Authorization: `Bearer ${t}` } })).status;
    }, { no: P, act });
  for (const act of ["submit", "approve", "revise"]) {
    const s = await lifecycle(P, act);
    if (s !== 200) throw new Error(`${act}: ${s}`);
  }
  const v2 = await setLines([{ line_no: 1, child: c2, qty: 5 }]);
  if (!v2.startsWith("204")) throw new Error(`set_lines v2: ${v2}`);

  // 详情页：live = 1 行（C2×5）——scoped 到 BOM 卡（页面还有版本历史表）
  await page.goto(`/docs/${P}`);
  await expect(page.locator('[data-test="bom-ver-select"]')).toBeVisible();
  const bomRows = page.locator('[data-test="bom-card"] .el-table__row');
  await expect(bomRows).toHaveCount(1);
  await expect(bomRows.getByRole("cell", { name: "5", exact: true })).toBeVisible();

  // 切 major=1 快照 → 2 行（v1 冻结形态）
  await page.click('[data-test="bom-ver-select"]');
  await page.click("text=v1.0 发布快照");
  await expect(bomRows).toHaveCount(2);

  // 增量15 终审 I1：被谁使用卡双视图（以 C2 视角——被 P 的 live 与两份发布快照使用）
  await page.goto(`/docs/${P}-C2`);
  await expect(page.locator('[data-test="wu-live-table"]')).toBeVisible();
  await expect(page.locator('[data-test="wu-live-table"] .el-table__row')).toHaveCount(1);
  await expect(page.locator('[data-test="wu-live-table"]').getByText(P)).toBeVisible();
  await expect(page.locator('[data-test="wu-asof-table"]')).toBeVisible();
  // 仅 v1 已发布（测试流程未发布 v2）→ 历史反查恰 1 行
  await expect(page.locator('[data-test="wu-asof-table"] .el-table__row')).toHaveCount(1, {
    timeout: 10_000,
  });
});

test("user admin: create user and grant role", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  await page.goto("/admin/users");
  await expect(page.locator('[data-test="role-admin-admin"]')).toBeVisible();

  const U = `bd${Date.now()}`;
  await page.click('[data-test="open-create"]');
  await page.fill('[data-test="new-username"]', U);
  await page.fill('[data-test="new-password"]', "bd-pass-123");
  await page.click('[data-test="create-submit"]');
  await expect(page.locator('[data-test="role-' + U + '-editor"]')).toBeVisible({ timeout: 10_000 });

  // 授予 editor（点击 tag 切换）
  await page.click(`[data-test="role-${U}-editor"]`);
  await expect(page.locator(`[data-test="role-${U}-editor"].el-tag--success`)).toBeVisible({ timeout: 10_000 });

  // 增量14 I2（裁定替代翻页钉——前端 size=50 翻页需 51 用户造数昂贵）：搜索联动分页条 total
  await page.fill('[data-test="user-search"]', U);
  await page.waitForTimeout(600); // 防抖 300ms + 加载
  await expect(page.locator(".el-pagination__total")).toContainText("共 1 条", { timeout: 10_000 });
  await expect(page.locator('[data-test="user-table"] .el-table__row')).toHaveCount(1);
});

// ==================== 增量12：M8 显隐 / URL 回写 ====================

test("role visibility: editor sees upload, viewer does not; neither sees admin menu", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");
  // admin 造号：editor0（editor）+ pureviewer（viewer）
  await page.evaluate(async () => {
    const t = localStorage.getItem("rustplm_token") ?? "";
    const api = (window as unknown as { E2E_API: string }).E2E_API;
    const mk = (username: string, roles: string[]) =>
      fetch(`${api}/api/v1/sec/users`, {
        method: "POST",
        headers: { Authorization: `Bearer ${t}`, "Content-Type": "application/json" },
        body: JSON.stringify({ username, password: "ux-pass-123", roles, max_secret_level: 3 }),
      });
    await mk("uxeditor", ["editor"]);
    await mk("uxviewer", ["viewer"]);
  });

  // editor：有上传按钮、无用户管理菜单
  await page.goto("/login");
  await page.fill('[data-test="username"]', "uxeditor");
  await page.fill('[data-test="password"]', "ux-pass-123");
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");
  await page.goto("/docs");
  await expect(page.locator('[data-test="open-upload"]')).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "用户管理" })).toHaveCount(0);

  // viewer：无上传按钮、无用户管理菜单、列表仍可读（M8 显隐；后端 casbin 仍权威）
  await page.goto("/login");
  await page.fill('[data-test="username"]', "uxviewer");
  await page.fill('[data-test="password"]', "ux-pass-123");
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");
  await page.goto("/docs");
  await expect(page.locator('[data-test="open-upload"]')).toHaveCount(0);
  await expect(page.getByRole("menuitem", { name: "用户管理" })).toHaveCount(0);
  await expect(page.locator(".el-pagination__total")).toHaveText(/\d+/);
});

test("state filter writes back to URL and survives reload", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");
  await page.goto("/docs");
  // el-select 非原生 select：点开下拉后选项点击
  await page.click('[data-test="state-filter"]');
  await page.getByRole("option", { name: "工作中" }).click();
  await page.waitForURL("**state=working**");
  // 刷新后筛选保持（增量11 M2 清偿钉）
  await page.reload();
  await page.waitForURL("**state=working**");
  await expect(page.locator(".el-pagination__total")).toHaveText(/\d+/);
});

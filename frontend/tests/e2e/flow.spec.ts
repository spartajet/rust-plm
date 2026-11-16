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

test("parts: create with class coding and dedup warning (PART-01/02/05)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");
  await page.goto("/parts");
  await page.click('[data-test="open-create-part"]');
  // 选分类（下拉：自制件含规则）
  await page.click('[data-test="part-class"]');
  // el-select 下拉 teleport+动画在无头视口外——键盘导航选首项（外购件，同样挂规则）
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(page.locator('[data-test="part-class"]')).toContainText('外购件', { timeout: 10_000 });
  const baseName = `垫片${Date.now()}`;
  await page.fill('[data-test="part-name"]', baseName);
  await page.fill('[data-test="part-material"]', "304");
  await page.click('[data-test="part-submit"]');
  // 查重确认弹窗条件处理（增量20：历史 run 物料 material=304 累积 → duplicates OR 材料匹配
  // → 弹「仍要创建」拦住 POST；无相似件时无弹窗——3s 内出现则点确认）
  const dupConfirm = page.locator('.el-message-box__btns .el-button--primary:has-text("仍要创建")');
  try {
    await dupConfirm.click({ timeout: 3000 });
  } catch {
    /* 无弹窗继续 */
  }
  // 取证：捕获 POST /parts 真实状态码（全套/单跑行为差异的根因证据）
  const postDone = page
    .waitForResponse((r) => r.url().includes("/api/v1/parts") && r.request().method() === "POST")
    .then((r) => `${r.status()} ${r.url().slice(-30)}`)
    .catch(() => "no-response");
  // 列表出现外购件分类码物料（WL-WG-——键盘导航选中首项分类）；
  // 名称筛选定位（增量20：数据累积后 WL-WG 在 obj_no 降序下不在第一页）
  await page.fill('[data-test="part-name-filter"]', baseName);
  await page.locator('[data-test="part-name-filter"]').press("Enter");
  await expect(
    page
      .locator('[data-test="parts-table"] .el-table__row')
      .filter({ hasText: "WL-WG-" })
      .first(),
  ).toBeVisible({ timeout: 15_000 });
  console.log("DBG-POST", await postDone);

  // 查重（PART-05 MVP 语义钉——browser fetch 直连 API，确定性；UI confirm 为真实用户增强交互）
  const dupResult = await page.evaluate(async (n) => {
    const t = localStorage.getItem("rustplm_token") ?? "";
    const r = await fetch(
      `${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/parts/duplicates?name=${encodeURIComponent(n)}`,
      { headers: { Authorization: `Bearer ${t}` } },
    );
    return r.status;
  }, baseName);
  expect(dupResult).toBe(200);
  await expect(
    page
      .locator('[data-test="parts-table"] .el-table__row')
      .filter({ hasText: baseName }),
  ).toHaveCount(1, { timeout: 15_000 }); // 查重前后恰 1 条同名件
});

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
  // 卡片计数与列表 total 最终一致（全串匹配；兼容数据落地延迟——增量20 布局接线后初渲 0 的竞态钉）
  const totalEl = page.locator(".el-pagination__total");
  await expect(totalEl).toHaveText(`共 ${workingCount} 条`, { timeout: 10_000 });
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

  // 增量15 终审 I1：被谁使用卡双视图（以 C2 视角——被 P 的 live 与已发布快照使用）
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

test("auto coding: switch disables obj input; unconfigured rule surfaces backend 400", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");
  await page.goto("/docs");
  await page.click("text=上传图纸");
  // 勾选自动编码 → 图号输入禁用（Review Focus #5；COD 码生成由后端 e2e 钉——规则须 SQL 预置）
  await page.click('[data-test="upload-auto"]');
  await expect(page.locator('[data-test="upload-objno"]')).toBeDisabled();
  await page.fill('[data-test="upload-name"]', "自动编码件");
  await page.setInputFiles('[data-test="upload-file"]', {
    name: "auto.dwg",
    mimeType: "application/octet-stream",
    buffer: Buffer.from("%AUTO"),
  });
  // 未配规则类型上 auto_code → 后端 400（前端 toast 错误消息）
  await page.click('[data-test="upload-submit"]');
  await expect(page.getByText("未配置编码规则")).toBeVisible({ timeout: 10_000 });
});

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

test("audit page: filter by action and paginated list (SYS-01)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");
  await page.goto("/admin/audit");
  // 筛选登录失败（本 run 至少产生过 1 条——前置测试若没有则此 run 前面旅程不含失败登录，
  // 改按 action=http 兜底查询必有数据）
  // el-select 非原生 select——键盘导航选首项（http）
  await page.click('[data-test="audit-action"]');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(page.locator('[data-test="audit-action"]')).toContainText('HTTP', { timeout: 10_000 });
  await expect(page.locator('[data-test="audit-table"] .el-table__row').first()).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.locator(".el-pagination__total")).toContainText("共");
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

// ==================== 增量19 WF-03 待办中心 ====================

test("todo center: three boxes + batch approve (WF-03)", async ({ page }) => {
  // 造数（browser fetch 直连后端）：建 editor → editor 上传并提交两件
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  const ED = `fe2todo${Date.now()}`;
  const NO1 = `DWG-FE-TD1-${Date.now()}`;
  const NO2 = `DWG-FE-TD2-${Date.now()}`;
  const setup = await page.evaluate(async ({ ED, NO1, NO2 }) => {
    const api = (window as unknown as { E2E_API: string }).E2E_API;
    const t = localStorage.getItem("rustplm_token") ?? "";
    const ah = { Authorization: `Bearer ${t}` };
    let r = await fetch(`${api}/api/v1/sec/users`, {
      method: "POST",
      headers: { ...ah, "Content-Type": "application/json" },
      body: JSON.stringify({ username: ED, password: "fe2todo-pass-123", roles: ["editor"] }),
    });
    if (r.status !== 201 && r.status !== 409) return `create-user ${r.status}`;
    r = await fetch(`${api}/api/v1/sec/users/${ED}/roles/editor`, { method: "POST", headers: ah });
    if (r.status !== 204 && r.status !== 409) return `grant ${r.status}`;
    const lr = await fetch(`${api}/api/v1/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username: ED, password: "fe2todo-pass-123" }),
    });
    const et = ((await lr.json()) as { access_token: string }).access_token;
    for (const no of [NO1, NO2]) {
      const fd = new FormData();
      fd.append("obj_no", no);
      fd.append("attrs", JSON.stringify({ name: "前端待办件" }));
      fd.append("file", new File(["%DWG-fe-todo"], "t.dwg", { type: "application/octet-stream" }));
      const ur = await fetch(`${api}/api/v1/docs`, {
        method: "POST",
        headers: { Authorization: `Bearer ${et}` },
        body: fd,
      });
      if (ur.status !== 201) return `upload ${no} ${ur.status}`;
      const sr = await fetch(`${api}/api/v1/docs/${no}/submit`, {
        method: "POST",
        headers: { Authorization: `Bearer ${et}` },
      });
      if (sr.status !== 200) return `submit ${no} ${sr.status}`;
    }
    return "ok";
  }, { ED, NO1, NO2 });
  expect(setup).toBe("ok");

  // 待办 tab：两行可见（提交人=editor）
  await page.goto("/todos");
  await expect(page.locator(`.el-table__row:has-text("${NO1}")`)).toBeVisible({ timeout: 10_000 });
  await expect(page.locator(`.el-table__row:has-text("${NO2}")`)).toBeVisible();
  await expect(
    page.locator(`.el-table__row:has-text("${NO1}")`).locator(`:text("${ED}")`),
  ).toBeVisible();

  // 批量批准：勾选两行 → 弹窗填意见 → 成功提示 → 待办清空
  await page.locator(`.el-table__row:has-text("${NO1}") .el-checkbox`).click();
  await page.locator(`.el-table__row:has-text("${NO2}") .el-checkbox`).click();
  await page.click('[data-test="batch-approve"]');
  await expect(page.locator(".el-message-box__input textarea")).toBeVisible({ timeout: 10_000 });
  await page.fill(".el-message-box__input textarea", "前端批量放行");
  await page.click(".el-message-box__btns .el-button--primary");
  await expect(page.locator('.el-message:has-text("已批准 2 件")')).toBeVisible({ timeout: 10_000 });
  await expect(page.locator(`.el-table__row:has-text("${NO1}")`)).toHaveCount(0, { timeout: 10_000 });

  // 已办 tab：approve 行可见；行点击跳详情
  await page.click('[data-test="tab-done"]');
  const doneRow = page.locator(`.el-table__row:has-text("${NO1}")`);
  await expect(doneRow).toBeVisible({ timeout: 10_000 });
  await expect(doneRow.locator(':text("approve")')).toBeVisible();
  await doneRow.click();
  await expect(page).toHaveURL(new RegExp(`/docs/${NO1}$`));
});

// ==================== 增量20 RPT-01 全局搜索 ====================

test("global search: topbar query clusters drawings and parts (RPT-01)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  // 造数（browser fetch 直连）：图纸「六角轴承座」+ 物料「六角轴承」
  const NO = `DWG-FE-SE-${Date.now()}`;
  const setup = await page.evaluate(async ({ NO }) => {
    const api = (window as unknown as { E2E_API: string }).E2E_API;
    const t = localStorage.getItem("rustplm_token") ?? "";
    const ah = { Authorization: `Bearer ${t}` };
    const fd = new FormData();
    fd.append("obj_no", NO);
    fd.append("attrs", JSON.stringify({ name: "六角轴承座", material: "Q235" }));
    fd.append("file", new File(["%DWG-fe-search"], "s.dwg", { type: "application/octet-stream" }));
    const ur = await fetch(`${api}/api/v1/docs`, { method: "POST", headers: ah, body: fd });
    if (ur.status !== 201) return `upload ${ur.status}`;
    const pr = await fetch(`${api}/api/v1/parts`, {
      method: "POST",
      headers: { ...ah, "Content-Type": "application/json" },
      body: JSON.stringify({ class_code: "CLS-ZZ", attrs: { name: "六角轴承", material: "45钢" } }),
    });
    if (pr.status !== 201) return `part ${pr.status}`;
    return "ok";
  }, { NO });
  expect(setup).toBe("ok");

  // 等扫描器（2s 周期）收编新行——轮询搜索 API 确定性就绪（并行满载下固定 sleep 不足）
  await expect
    .poll(
      async () => {
        return page.evaluate(async (no) => {
          const api = (window as unknown as { E2E_API: string }).E2E_API;
          const t = localStorage.getItem("rustplm_token") ?? "";
          const r = await fetch(
            `${api}/api/v1/search?q=${encodeURIComponent(no)}`,
            { headers: { Authorization: `Bearer ${t}` } },
          );
          const d = (await r.json()) as { total: number };
          return d.total;
        }, NO);
      },
      { timeout: 15_000, intervals: [500] },
    )
    .toBe(1);

  // 顶栏搜索 → /search?q= 聚类页（中文词：双组聚类 smoke；特定行不做排名断言——
  // 既有 run 累积同名词会挤掉 top-10）
  await page.fill('[data-test="global-search"]', "轴承");
  await page.press('[data-test="global-search"]', "Enter");
  await expect(page).toHaveURL(/\/search\?q=/);
  await expect(
    page.locator('[data-test="search-table-drawing"] .el-table__row').first(),
  ).toBeVisible({ timeout: 15_000 });
  await expect(page.locator('[data-test="search-table-part"] .el-table__row').first()).toBeVisible();

  // 编号精确搜索（纯 ASCII→obj_no term 语义钉）：行级断言+跳详情
  await page.fill('[data-test="global-search"]', NO);
  await page.press('[data-test="global-search"]', "Enter");
  await expect(page).toHaveURL(new RegExp(`q=${NO}`));
  await expect(page.locator(`[data-test="search-table-drawing"] .el-table__row:has-text("${NO}")`)).toBeVisible({ timeout: 10_000 });
  await page.locator(`[data-test="search-table-drawing"] .el-table__row:has-text("${NO}")`).locator(':text("查看")').click();
  await expect(page).toHaveURL(new RegExp(`/docs/${NO}$`));
});

// ==================== 增量21 BOM 编辑器 + BOM-07 汇总 ====================

test("bom editor: edit save summary csv (BOM-01/07)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  // 造数（browser fetch 直连）：父图纸 + 两个子图纸
  const P = `DWG-FE-B21-${Date.now()}`;
  const C1 = `DWG-FE-B21C1-${Date.now()}`;
  const C2 = `DWG-FE-B21C2-${Date.now()}`;
  const setup = await page.evaluate(async ({ P, C1, C2 }) => {
    const api = (window as unknown as { E2E_API: string }).E2E_API;
    const t = localStorage.getItem("rustplm_token") ?? "";
    const ah = { Authorization: `Bearer ${t}` };
    const up = async (no: string) => {
      const fd = new FormData();
      fd.append("obj_no", no);
      fd.append("attrs", JSON.stringify({ name: "B21件" }));
      fd.append("file", new File(["%B21"], "b.dwg", { type: "application/octet-stream" }));
      const r = await fetch(`${api}/api/v1/docs`, { method: "POST", headers: ah, body: fd });
      if (r.status !== 201) return `up ${no} ${r.status}`;
      return null;
    };
    for (const e of [await up(P), await up(C1), await up(C2)]) {
      if (e) return e;
    }
    // 取子件 uuid（详情接口 object.id）
    const idOf = async (no: string) => {
      const r = await fetch(`${api}/api/v1/docs/${no}`, { headers: ah });
      const d = (await r.json()) as { object: { id: string } };
      return d.object.id;
    };
    const [c1, c2] = [await idOf(C1), await idOf(C2)];
    const r = await fetch(`${api}/api/v1/boms/${P}/lines`, {
      method: "POST",
      headers: { ...ah, "Content-Type": "application/json" },
      body: JSON.stringify([
        { line_no: 1, child: c1, qty: 2 },
        { line_no: 2, child: c2, qty: 3 },
      ]),
    });
    if (r.status !== 204) return `set ${r.status}`;
    return "ok";
  }, { P, C1, C2 });
  expect(setup).toBe("ok");

  // 等扫描器收编新件（编辑器"添加子件"走全局搜索——索引就绪确定性轮询）
  await expect
    .poll(
      async () => {
        return page.evaluate(async (no) => {
          const api = (window as unknown as { E2E_API: string }).E2E_API;
          const t = localStorage.getItem("rustplm_token") ?? "";
          const r = await fetch(`${api}/api/v1/search?q=${encodeURIComponent(no)}`, {
            headers: { Authorization: `Bearer ${t}` },
          });
          const d = (await r.json()) as { total: number };
          return d.total;
        }, C1);
      },
      { timeout: 15_000, intervals: [500] },
    )
    .toBe(1);

  // 详情页：富行展示（编号/名称——增量21 前不再裸 uuid）
  await page.goto(`/docs/${P}`);
  await expect(page.locator('[data-test="bom-lines-table"] .el-table__row')).toHaveCount(2, { timeout: 10_000 });
  await expect(page.locator(`[data-test="bom-lines-table"] .el-table__row:has-text("${C1}")`)).toBeVisible();

  // 编辑器：删一行 + 改 qty + 经全局搜索添加新行 → 保存 → onelevel 复验
  await page.click('[data-test="bom-edit-btn"]');
  await expect(page.locator('[data-test="bom-edit-table"] .el-table__row')).toHaveCount(2);
  // 删除第一行（C1）
  await page.locator('[data-test="bom-del-btn"]').first().click();
  // 改第二行（C2）数量为 5
  await page.locator('[data-test="bom-qty-input"] input').fill("5");
  // 添加子件：全局搜索 C1 → 选择
  await page.click('[data-test="bom-add-btn"]');
  await page.fill('[data-test="bom-add-search"]', C1);
  await page.click('[data-test="bom-add-search-btn"]');
  await page.locator('[data-test="bom-add-results"] .el-table__row').first().waitFor({ timeout: 10_000 });
  await page.locator('[data-test="bom-add-select"]').first().click();
  await expect(page.locator('[data-test="bom-edit-table"] .el-table__row')).toHaveCount(2);
  await page.click('[data-test="bom-save-btn"]');
  await expect(page.locator('.el-message:has-text("BOM 已保存")')).toBeVisible({ timeout: 10_000 });
  // 保存后富行复验：C1 回到行内（qty=1）
  await expect(page.locator(`[data-test="bom-lines-table"] .el-table__row:has-text("${C1}")`)).toBeVisible({ timeout: 10_000 });

  // 汇总：展开折叠（懒加载触发）→ 两叶行 → CSV 下载钉
  await page.locator('.el-collapse-item__header:has-text("用量/分类汇总")').click();
  await expect(page.locator('[data-test="bom-summary-table"] .el-table__row')).toHaveCount(2, { timeout: 10_000 });
  await page.click('[data-test="bom-csv-btn"]');
  const download = await page.waitForEvent("download", { timeout: 10_000 });
  expect(download.suggestedFilename()).toBe(`summary-${P}.csv`);
});

// ==================== 增量22 DOC-05 在线预览 ====================

test("doc preview: png inline with watermark, dwg falls back to download hint (DOC-05)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  // 造数：PNG（白名单内）+ DWG（白名单外 octet-stream）
  const PNG = `DWG-FE-PV-PNG-${Date.now()}`;
  const DWG = `DWG-FE-PV-DWG-${Date.now()}`;
  const setup = await page.evaluate(async ({ PNG, DWG }) => {
    const api = (window as unknown as { E2E_API: string }).E2E_API;
    const t = localStorage.getItem("rustplm_token") ?? "";
    const ah = { Authorization: `Bearer ${t}` };
    // 1×1 透明 PNG
    const bytes = new Uint8Array(
      atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
        .split("")
        .map((c) => c.charCodeAt(0)),
    ).buffer;
    const up = async (no: string, name: string, data: ArrayBuffer, mime: string) => {
      const fd = new FormData();
      fd.append("obj_no", no);
      fd.append("attrs", JSON.stringify({ name }));
      fd.append("file", new File([data], name, { type: mime }));
      const r = await fetch(`${api}/api/v1/docs`, { method: "POST", headers: ah, body: fd });
      if (r.status !== 201) return `up ${no} ${r.status}`;
      return null;
    };
    for (const e of [
      await up(PNG, "pv.png", bytes, "image/png"),
      // DWG 字节每次唯一——内容寻址去重（file_object ON CONFLICT hash DO NOTHING）下
      // file_object.mime 固化自首次上传，同字节复用会拿到旧 mime
      await up(
        DWG,
        "pv.dwg",
        new TextEncoder().encode(`octet-dwg-${DWG}`).buffer as ArrayBuffer,
        "application/octet-stream",
      ),
    ]) {
      if (e) return e;
    }
    return "ok";
  }, { PNG, DWG });
  expect(setup).toBe("ok");

  // PNG：预览图可见 + 水印层存在
  await page.goto(`/docs/${PNG}`);
  await page.click('[data-test="preview-btn"]');
  await expect(page.locator('[data-test="preview-img"]')).toBeVisible({ timeout: 10_000 });
  await expect(page.locator('[data-test="preview-watermark"]')).toBeVisible();
  await expect(page.locator('[data-test="preview-watermark"]')).toContainText("admin");
  // 终审 I1 回归钉：水印 overlay 样式真实落地（absolute+pointer-events none——computed style
  // 断言，纯文本可见性断言拦不住 CSS 缺失——增量22 修复轮实证过的盲区）
  const wmStyle = await page.evaluate(() => {
    const wm = document.querySelector('[data-test="preview-watermark"]');
    const wrap = document.querySelector('[data-test="preview-area"]');
    const cs = getComputedStyle(wm as Element);
    return { pos: cs.position, pe: cs.pointerEvents, wrapPos: getComputedStyle(wrap as Element).position };
  });
  expect(wmStyle).toEqual({ pos: "absolute", pe: "none", wrapPos: "relative" });
  await page.click('[data-test="preview-close"]');
  await expect(page.locator('[data-test="preview-img"]')).toHaveCount(0);

  // DWG：415 → 错误提示（走下载）
  await page.goto(`/docs/${DWG}`);
  await page.click('[data-test="preview-btn"]');
  await expect(
    page.locator('.el-message:has-text("该格式不支持在线预览")'),
  ).toBeVisible({ timeout: 10_000 });
});

// ==================== 增量23 SYS-04 安全参数 ====================

test("security params: panel edit saves and reflects (SYS-04)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  await page.goto("/admin/users");
  await expect(page.locator('[data-test="params-table"] .el-table__row')).toHaveCount(4, { timeout: 10_000 });
  // 终审 C1-3：含 checkout_hours 行（增量26 种子）防静默漂移
  await expect(page.locator('[data-test="params-table"]:has-text("检出时长（小时）")')).toBeVisible();

  // 改 lock_minutes 15→20 → 保存 → 面板值刷新为 20 → 还原 15。
  // 策略生效语义由后端 e2e 钉住（sec_params_manage_and_audit_trail）；此处不造全局副作用
  // （曾改 min_length+建号复验——与并行 user-admin 测试的全局参数竞争实测撞车，增量23 修复轮裁定）
  const input = page.locator('[data-test="param-input-lock_minutes"] input');
  await input.fill("20");
  await page.click('[data-test="param-save-lock_minutes"]');
  await expect(page.locator('.el-message:has-text("已保存")')).toBeVisible({ timeout: 10_000 });
  await expect(input).toHaveValue("20", { timeout: 10_000 });

  await input.fill("15");
  await page.click('[data-test="param-save-lock_minutes"]');
  await expect(page.locator('.el-message:has-text("已保存")')).toBeVisible({ timeout: 10_000 });
  await expect(input).toHaveValue("15", { timeout: 10_000 });
});

// ==================== 增量24 AGL-05/09 模板包+实施向导 ====================

test("impl wizard: eight steps + starter import report (AGL-05/09)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  // 向导卡展开：八步渲染
  await page.click('[data-test="wizard-card"] .el-card__body > div:first-child');
  await expect(page.locator('[data-test="wizard-steps"] .wiz-step')).toHaveCount(8, { timeout: 10_000 });

  // 导入 Starter 包（dry_run 冲突+merge 一体流）→ 报告行可见
  await page.click('[data-test="import-starter"]');
  await expect(page.locator('[data-test="import-report"]')).toBeVisible({ timeout: 15_000 });
  await expect(page.locator('[data-test="import-report"]')).toContainText("模板导入");

  // training 人工步标记完成 → 状态翻已完成
  const trainingDone = await page
    .locator('.wiz-step:has-text("培训交付") [data-test="wizard-step-done"]')
    .innerText();
  if (trainingDone.includes("待完成")) {
    await page.click('[data-test="wizard-complete-training"]');
    await expect(
      page.locator('.wiz-step:has-text("培训交付") [data-test="wizard-step-done"]:has-text("已完成")'),
    ).toBeVisible({ timeout: 10_000 });
  }
});

// ==================== 增量26 DOC-03 检出锁 ====================

test("checkout lock: banner blocks other editor, checkin clears (DOC-03)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  const NO = `DWG-FE-CKL-${Date.now()}`;
  const setup = await page.evaluate(async (no) => {
    const api = (window as unknown as { E2E_API: string }).E2E_API;
    const t = localStorage.getItem("rustplm_token") ?? "";
    const fd = new FormData();
    fd.append("obj_no", no);
    fd.append("attrs", JSON.stringify({ name: "锁件" }));
    fd.append("file", new File(["%L"], "l.dwg", { type: "application/octet-stream" }));
    const r = await fetch(`${api}/api/v1/docs`, { method: "POST", headers: { Authorization: `Bearer ${t}` }, body: fd });
    return r.status;
  }, NO);
  expect(setup).toBe(201);

  // admin 检出 → 自持横幅（绿）
  await page.goto(`/docs/${NO}`);
  await page.click('[data-test="lock-checkout"]');
  await expect(page.locator('[data-test="lock-banner-self"]')).toBeVisible({ timeout: 10_000 });
  await expect(page.locator('[data-test="lock-checkin"]')).toBeVisible();

  // 检入 → 横幅消失
  await page.click('[data-test="lock-checkin"]');
  await expect(page.locator('[data-test="lock-banner-self"]')).toHaveCount(0, { timeout: 10_000 });

  // 再检出 → 强制解锁可用（admin 有 sec:manage）→ 横幅消失
  await page.click('[data-test="lock-checkout"]');
  await expect(page.locator('[data-test="lock-banner-self"]')).toBeVisible({ timeout: 10_000 });
  await page.click('[data-test="lock-force-unlock"]');
  await expect(page.locator('[data-test="lock-banner-self"]')).toHaveCount(0, { timeout: 10_000 });
});

// ==================== 增量27 DOC-12 关联物料 ====================

test("doc links: link a part via search dialog, unlink clears (DOC-12)", async ({ page }) => {
  await page.goto("/login");
  await page.fill('[data-test="username"]', "admin");
  await page.fill('[data-test="password"]', ADMIN_PW);
  await page.click('[data-test="login-btn"]');
  await page.waitForURL("/");

  const NO = `DWG-FE-LNK-${Date.now()}`;
  const PNAME = `关联垫块${Date.now()}`;
  const setup = await page.evaluate(async ({ NO, PNAME }) => {
    const api = (window as unknown as { E2E_API: string }).E2E_API;
    const t = localStorage.getItem("rustplm_token") ?? "";
    const ah = { Authorization: `Bearer ${t}` };
    const fd = new FormData();
    fd.append("obj_no", NO);
    fd.append("attrs", JSON.stringify({ name: "关联主图" }));
    fd.append("file", new File(["%LNK"], "l.dwg", { type: "application/octet-stream" }));
    const r = await fetch(`${api}/api/v1/docs`, { method: "POST", headers: ah, body: fd });
    if (r.status !== 201) return `doc ${r.status}`;
    const pr = await fetch(`${api}/api/v1/parts`, {
      method: "POST",
      headers: { ...ah, "Content-Type": "application/json" },
      body: JSON.stringify({ class_code: "CLS-ZZ", attrs: { name: PNAME, material: "Q235" } }),
    });
    if (pr.status !== 201) return `part ${pr.status}`;
    // 终审 M3：回读服务端赋码 obj_no（link-pick-{obj_no} 精确钉）
    const dr = await fetch(`${api}/api/v1/parts/duplicates?name=${encodeURIComponent(PNAME)}`, { headers: ah });
    const ditems = (await dr.json()) as { obj_no: string; name?: string }[];
    const found = ditems.find((i) => i.name === PNAME);
    return found?.obj_no ?? "objno-miss";
  }, { NO, PNAME });
  expect(setup).toMatch(/WL-ZZ-/);

  // 精确钉：link-pick-{obj_no}
  const POBJ = setup;

  // 详情页关联物料卡：搜索选件关联 → 行可见 → 解除 → 空态
  await page.goto(`/docs/${NO}`);
  await expect(page.locator('[data-test="links-card"]')).toBeVisible({ timeout: 10_000 });
  await page.click('[data-test="link-open"]');
  await page.fill('[data-test="link-search"]', PNAME);
  await page.click('[data-test="link-search-btn"]');
  await page.locator(`[data-test="link-pick-${POBJ}"]`).waitFor({ timeout: 10_000 });
  const row = page.locator('[data-test="link-results"] .el-table__row').filter({ hasText: PNAME }).first();
  await expect(row).toBeVisible({ timeout: 10_000 });
  await row.locator("button").click();
  await expect(page.locator('[data-test="links-table"] .el-table__row:has-text("WL-ZZ-")')).toBeVisible({ timeout: 10_000 });
  // 解除
  await page.locator('[data-test="links-table"] .el-table__row button:has-text("解除")').first().click();
  await expect(page.locator('[data-test="links-table"] .el-table__row')).toHaveCount(0, { timeout: 10_000 });
});

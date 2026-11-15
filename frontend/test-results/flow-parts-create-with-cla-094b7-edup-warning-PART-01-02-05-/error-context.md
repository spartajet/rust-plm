# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: flow.spec.ts >> parts: create with class coding and dedup warning (PART-01/02/05)
- Location: tests\e2e\flow.spec.ts:23:1

# Error details

```
Test timeout of 60000ms exceeded.
```

```
Error: page.evaluate: Target page, context or browser has been closed
```

# Page snapshot

```yaml
- generic [ref=f1e1]:
  - generic [ref=f1e3]:
    - generic [ref=f1e4]:
      - button "新建物料" [ref=f1e5] [cursor=pointer]
      - button "刷新" [ref=f1e7] [cursor=pointer]
    - generic [ref=f1e10]:
      - table [ref=f1e12]:
        - rowgroup [ref=f1e20]:
          - row [ref=f1e21]:
            - columnheader "物料号" [ref=f1e22]
            - columnheader "名称" [ref=f1e24]
            - columnheader "分类" [ref=f1e26]
            - columnheader "材料" [ref=f1e28]
            - columnheader "规格" [ref=f1e30]
            - columnheader "版本" [ref=f1e32]
      - table [ref=f1e38]:
        - rowgroup [ref=f1e46]:
          - row [ref=f1e47]:
            - cell "WL-WG-00001" [ref=f1e48]
            - cell "垫片1794725390528" [ref=f1e50]
            - cell "外购件" [ref=f1e52]
            - cell "304" [ref=f1e54]
            - cell "-" [ref=f1e56]
            - cell "v1.0" [ref=f1e58]
    - generic [ref=f1e60]:
      - generic [ref=f1e61]: 共 1 条
      - button "上一页" [disabled] [ref=f1e62]
      - list [ref=f1e63]:
        - listitem "第 1 页" [ref=f1e64]: "1"
      - button "下一页" [disabled] [ref=f1e65]
    - dialog "新建物料" [ref=f1e67]:
      - generic [ref=f1e68]:
        - banner [ref=f1e69]:
          - heading "新建物料" [level=2] [ref=f1e70]
          - button "关闭此对话框" [ref=f1e71] [cursor=pointer]
        - generic [ref=f1e76]:
          - generic [ref=f1e77]:
            - generic [ref=f1e78]: 分类
            - generic [ref=f1e81] [cursor=pointer]:
              - generic:
                - combobox "分类" [ref=f1e83]
                - generic [ref=f1e84]: 外购件（CLS-WG）
          - generic [ref=f1e89]:
            - generic [ref=f1e90]: 名称
            - textbox "名称" [ref=f1e94]: 垫片1794725710290
          - generic [ref=f1e95]:
            - generic [ref=f1e96]: 材料
            - textbox "材料" [ref=f1e100]:
              - /placeholder: 如 Q235 / 304
              - text: "304"
          - generic [ref=f1e101]:
            - generic [ref=f1e102]: 规格
            - textbox "规格" [ref=f1e106]
        - contentinfo [ref=f1e107]:
          - button "取消" [ref=f1e108] [cursor=pointer]
          - button "创建（自动赋码）" [disabled]
  - dialog "查重提示" [ref=f1e111]:
    - generic [ref=f1e112]:
      - generic [ref=f1e113]:
        - generic [ref=f1e114]: 查重提示
        - button "关闭此对话框" [ref=f1e115] [cursor=pointer]
      - paragraph [ref=f1e125]: 发现 1 个相似件（WL-WG-00001），仍要创建吗？
      - generic [ref=f1e126]:
        - button "取消" [ref=f1e127] [cursor=pointer]
        - button "仍要创建" [active] [ref=f1e129] [cursor=pointer]
```

# Test source

```ts
  1   | import { expect, test } from "@playwright/test";
  2   | 
  3   | /**
  4   |  * 真链路 e2e（增量7 Task4）。
  5   |  * 前置（脚本外部准备）：
  6   |  *   - 后端 plm-server 跑在 http://localhost:18095（测试库，PLM_ADMIN_PASSWORD=e2e-pass-123）
  7   |  *   - vite dev 跑在 http://localhost:5173（VITE_API_BASE=http://localhost:18095）
  8   |  * 链路：登录 → 上传图纸 → 列表可见 → 详情 → submit → approve(admin) → released
  9   |  */
  10  | 
  11  | const ADMIN_PW = process.env.E2E_ADMIN_PASSWORD ?? "e2e-pass-123";
  12  | // chromium 内 fetch 直连后端（绕开 vite proxy 的 node RST 通道；CORS 已放开）。
  13  | // 经 addInitScript 注入 window.E2E_API——page.evaluate 闭包看不到 Node 作用域
  14  | const API = process.env.E2E_API_BASE ?? "http://127.0.0.1:18095";
  15  | test.beforeEach(async ({ page }) => {
  16  |   await page.addInitScript((api: string) => {
  17  |     (window as unknown as { E2E_API: string }).E2E_API = api;
  18  |   }, API);
  19  | });
  20  | // 终审 I5：唯一 obj_no 保证 e2e 可重复执行
  21  | const OBJ = `DWG-E2E-FE-${Date.now()}`;
  22  | 
  23  | test("parts: create with class coding and dedup warning (PART-01/02/05)", async ({ page }) => {
  24  |   await page.goto("/login");
  25  |   await page.fill('[data-test="username"]', "admin");
  26  |   await page.fill('[data-test="password"]', ADMIN_PW);
  27  |   await page.click('[data-test="login-btn"]');
  28  |   await page.waitForURL("/");
  29  |   await page.goto("/parts");
  30  |   await page.click('[data-test="open-create-part"]');
  31  |   // 选分类（下拉：自制件含规则）
  32  |   await page.click('[data-test="part-class"]');
  33  |   // el-select 下拉 teleport+动画在无头视口外——键盘导航选首项（外购件，同样挂规则）
  34  |   await page.keyboard.press('ArrowDown');
  35  |   await page.keyboard.press('Enter');
  36  |   await expect(page.locator('[data-test="part-class"]')).toContainText('外购件', { timeout: 10_000 });
  37  |   const baseName = `垫片${Date.now()}`;
  38  |   await page.fill('[data-test="part-name"]', baseName);
  39  |   await page.fill('[data-test="part-material"]', "304");
  40  |   await page.click('[data-test="part-submit"]');
  41  |   // 取证：捕获 POST /parts 真实状态码（全套/单跑行为差异的根因证据）
  42  |   const postDone = page
  43  |     .waitForResponse((r) => r.url().includes("/api/v1/parts") && r.request().method() === "POST")
  44  |     .then((r) => `${r.status()} ${r.url().slice(-30)}`)
  45  |     .catch(() => "no-response");
  46  |   // 列表出现外购件分类码物料（WL-WG-——键盘导航选中首项分类）
  47  |   await expect(
  48  |     page.locator('[data-test="parts-table"] .el-table__row:has-text("WL-WG-")').first(),
  49  |   ).toBeVisible({ timeout: 15_000 });
  50  |   console.log("DBG-POST", await postDone);
  51  | 
  52  |   // 查重（PART-05 MVP 语义钉——browser fetch 直连 API，确定性；UI confirm 为真实用户增强交互）
> 53  |   const dupResult = await page.evaluate(async (n) => {
      |                                ^ Error: page.evaluate: Target page, context or browser has been closed
  54  |     const t = localStorage.getItem("rustplm_token") ?? "";
  55  |     const r = await fetch(
  56  |       `${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/parts/duplicates?name=${encodeURIComponent(n)}`,
  57  |       { headers: { Authorization: `Bearer ${t}` } },
  58  |     );
  59  |     return r.status;
  60  |   }, baseName);
  61  |   expect(dupResult).toBe(200);
  62  |   await expect(
  63  |     page
  64  |       .locator('[data-test="parts-table"] .el-table__row')
  65  |       .filter({ hasText: baseName }),
  66  |   ).toHaveCount(1, { timeout: 15_000 }); // 查重前后恰 1 条同名件
  67  | });
  68  | 
  69  | test("full journey: login → upload → list → detail → lifecycle", async ({ page }) => {
  70  |   // 1) 登录
  71  |   await page.goto("/login");
  72  |   await page.fill('[data-test="username"]', "admin");
  73  |   await page.fill('[data-test="password"]', ADMIN_PW);
  74  |   await page.click('[data-test="login-btn"]');
  75  |   await page.waitForURL("/");
  76  | 
  77  |   // 2) 进入图纸页 → 打开上传对话框
  78  |   await page.goto("/docs");
  79  |   await page.click("text=上传图纸");
  80  |   await page.fill('[data-test="upload-objno"]', OBJ);
  81  |   await page.fill('[data-test="upload-name"]', "前端E2E支架");
  82  |   await page.fill('[data-test="upload-material"]', "Q235");
  83  |   await page.setInputFiles('[data-test="upload-file"]', {
  84  |     name: "fe.dwg",
  85  |     mimeType: "application/octet-stream",
  86  |     buffer: Buffer.from("%DWG-FRONTEND-E2E"),
  87  |   });
  88  |   await page.click('[data-test="upload-submit"]');
  89  |   // toast 3s 即逝不作断言；等刷新后的行出现（Review Focus #4）
  90  |   await expect(page.getByRole("cell", { name: OBJ })).toBeVisible({ timeout: 15_000 });
  91  | 
  92  |   // 4) 详情：属性与状态
  93  |   await page.goto(`/docs/${OBJ}`);
  94  |   await expect(page.getByText("前端E2E支架")).toBeVisible();
  95  |   await expect(page.locator('[data-test="state-tag"]')).toHaveText("工作中");
  96  | 
  97  |   // 5) 生命周期：submit → review → approve → released（Review Focus #5）
  98  |   await page.click('[data-test="act-submit"]');
  99  |   await expect(page.locator('[data-test="state-tag"]')).toHaveText("审核中", { timeout: 10_000 });
  100 |   await page.click('[data-test="act-approve"]');
  101 |   await expect(page.locator('[data-test="state-tag"]')).toHaveText("已发布", { timeout: 10_000 });
  102 | 
  103 |   // 6) released 态：submit 按钮不再显示；版本徽章 v1.0
  104 |   await expect(page.locator('[data-test="act-submit"]')).toHaveCount(0);
  105 |   await expect(page.locator('[data-test="ver-badge"]')).toHaveText("v1.0");
  106 | 
  107 |   // 7) 升版（增量8）：released → 升新版 → working v2.0（Review Focus #6）
  108 |   await page.click('[data-test="act-revise"]');
  109 |   await expect(page.locator('[data-test="state-tag"]')).toHaveText("工作中", { timeout: 10_000 });
  110 |   await expect(page.locator('[data-test="ver-badge"]')).toHaveText("v2.0");
  111 |   // 版本历史卡有 v1.0 发布快照
  112 |   await expect(page.getByText("版本历史（发布快照）")).toBeVisible();
  113 |   await expect(page.getByRole("cell", { name: "v1.0" })).toBeVisible();
  114 | });
  115 | 
  116 | test("guard: unauthenticated /docs redirects to login", async ({ page }) => {
  117 |   // 终审 C1：守卫接线钉——未认证直接访问受保护页必须跳 /login
  118 |   await page.goto("/docs");
  119 |   await page.waitForURL("**/login", { timeout: 10_000 });
  120 |   await expect(page.locator('[data-test="login-btn"]')).toBeVisible();
  121 | });
  122 | 
  123 | // ==================== 增量11：看板 / BOM 版本切换 / 用户管理 ====================
  124 | // （造数一律走 page.evaluate 内的 browser fetch——chromium 直连 API，规避本机 node 环回 RST）
  125 | 
  126 | test("board: per-state counts match and click links to filtered list", async ({ page }) => {
  127 |   await page.goto("/login");
  128 |   await page.fill('[data-test="username"]', "admin");
  129 |   await page.fill('[data-test="password"]', ADMIN_PW);
  130 |   await page.click('[data-test="login-btn"]');
  131 |   await page.waitForURL("/");
  132 | 
  133 |   // 造 1 个 review 对象（此时 journey 已留 1 working；released 已 revise 回 working）
  134 |   const OBJ = `DWG-BOARD-${Date.now()}`;
  135 |   const up = await page.evaluate(async (no) => {
  136 |     const t = localStorage.getItem("rustplm_token") ?? "";
  137 |     const fd = new FormData();
  138 |     fd.set("obj_no", no);
  139 |     fd.set("attrs", JSON.stringify({ name: "Board件" }));
  140 |     fd.set("file", new File(["%BOARD"], "b.dwg"));
  141 |     const r = await fetch(`${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/docs`, {
  142 |       method: "POST",
  143 |       headers: { Authorization: `Bearer ${t}` },
  144 |       body: fd,
  145 |     });
  146 |     return r.status;
  147 |   }, OBJ);
  148 |   if (up !== 201) throw new Error(`upload failed: ${up}`);
  149 |   const submitted = await page.evaluate(async (no) => {
  150 |     const t = localStorage.getItem("rustplm_token") ?? "";
  151 |     return (
  152 |       await fetch(`${(window as unknown as { E2E_API: string }).E2E_API}/api/v1/docs/${no}/submit`, {
  153 |         method: "POST",
```
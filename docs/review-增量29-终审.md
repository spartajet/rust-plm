# 增量29 终审报告——UI 设计系统重设计（Industrial Slate 明暗双主题）

- 分支：`feat/ui-redesign`（基线 main @ 1cb1855，增量28 合并+CI 修复后；任务声明 5a3ec43 系增量28 Task5 文档点，实际 merge-base 为 1cb1855，差异仅为增量28 CI 修复提交，无对账影响）
- 审查范围：d5a0155（计划）/ 75529ea（T1+T2+前置自修+T3）/ 7d20f04（T4 文档）
- 审查方式：fresh eyes 独立终审——git show 三 commit 全读 + 全部新增/改动文件逐行 + 本地全量实跑（后端/前端/真库探针/Playwright 全量复跑）+ 构建产物 CSS 顺序取证 + 运行时 computed style 探针与截图取证
- 日期：2026-11-16

---

## 总判定：**FIX-THEN-MERGE**（C1×2，M×1，I×5）

增量29 的主题机制骨架（tokens 双主题/useTheme 持久化/Element dark 桥接/图标导航/adminOnly 显隐）代码质量合格、既有 19 钉基本零破坏；但**两项 C1** 必须先修：

1. **声明的 list_params "正向白名单修复"不存在于代码**——demo 库上 demo_seeded 仍泄漏进 GET /sec/params（实测 5 行），security params 钉实跑 3/3 次失败，"Playwright 19×2 全绿"声明失实；
2. **main.css 后导入覆盖 body 背景与字体**——暗色下主内容画布仍是亮灰（运行时实测 body=rgb(245,247,250)），"明暗两套视觉完整"验收不成立（截图取证）。

两处最小修复均为 1–3 行，修后补跑 demo 库 security params 钉 + 暗色画布目检即可转 PASS。

---

## C1（必须修，合并阻断）

### C1-1 声明失实 + 功能回归未修：list_params "正向白名单 4 键 IN"不在代码中，demo_seeded 仍泄漏（security params 钉实跑必败）

**声明**（75529ea 提交信息 + 计划台账 `docs/superpowers/plans/2026-11-16-ui-redesign.md:49`）：
> "②list_params 负向过滤漏 demo_seeded（security params 钉 5≠4 拦下→改正向白名单 4 键——内部键一劳永逸）"

**代码实况（三重取证）**：
- `crates/core-sec/src/service.rs:466-469`：`params()` 仍是负向单键过滤——
  `SELECT key, value, updated_by, updated_at FROM sys_param WHERE key <> 'impl_checklist_done' ORDER BY key`
  无任何 `key IN (...)` 白名单；
- `git diff main --stat`：全分支 12 个改动文件全部为 frontend/docs/package.json，**后端零改动**——与声明"改正向白名单"直接矛盾（也与"基线：后端零改动"自相打架：既声称改了 list_params，又声称后端零改动，二者必有一假——实测前者假）；
- `git log --all -S "key IN ('password_min_length'"`：全仓库全历史无此修复痕迹。

**运行时实证（demo 库 plm_rv29_e2e，PLM_DEMO_SEED=1）**：
```
GET /api/v1/sec/params（admin）→ rows= 5
 - checkout_hours = 24 / - demo_seeded = 1 / - lock_minutes = 15 / - lock_threshold = 5 / - password_min_length = 8
```
demo_seeded 写入点在 `crates/plm-server/src/seed.rs:20,327-330`（INSERT INTO sys_param），负向过滤拦不住它。

**Playwright 全量复跑实况**（plm_rv29_e2e @18095 + vite 5173，20 钉）：`security params: panel edit saves and reflects (SYS-04)` 在 `frontend/tests/e2e/flow.spec.ts:741` `toHaveCount(4)` 上 **3/3 次 FAIL（Expected: 4, Received: 5）**；其余 19 钉全过（1 个 parts 钉首跑环境抖动 retry 过，与 config 既有裁定一致）→ 声明"Playwright 19×2 全绿"失实（实测 19 过/1 败）。

**影响**：demo 部署的参数面板出现一行无意义、可"编辑保存"的 demo_seeded 行（PUT 虽被 RANGES 400 拦截，但 GET 面板渲染混入内部键）；更根本的是**对账红线被击穿**——被钉拦下的回归声明已修，实际未修。

**最小修复**（一行 SQL，与网关白名单同源收敛）：
```sql
-- crates/core-sec/src/service.rs params()
WHERE key IN ('password_min_length','lock_threshold','lock_minutes','checkout_hours') ORDER BY key
```
（与 `crates/api-gateway/src/routes/sec_params.rs:24` RANGES 4 键严格一致；内部键 impl_checklist_done/demo_seeded 一劳永逸隔离。）修后于 demo 库补跑 security params 钉确认 4 行。

### C1-2 暗色视觉回归：main.css 在 tokens/components 之后导入，body 背景/字体被硬编码亮色覆盖——暗色下主内容画布仍为亮灰

**根因**：`frontend/src/main.ts:16` 的 `import "./styles/main.css"`（既有行）现在落在新增的 `tokens.css`(13)/`components.css`(14) **之后**；`frontend/src/styles/main.css` 的
```css
body { margin: 0; font-family: "Helvetica Neue", Arial, ...; background: #f5f7fa; }
```
与 components.css 的 `body { font-family: var(--rp-font); background: var(--rp-bg); color: var(--rp-text); }` 同特异性（0,0,1），**后导入者胜** → body 背景恒为 #f5f7fa、字体恒为旧 Helvetica 栈。

**取证**：
- 构建产物 `frontend/dist/assets/index-*.css`：components.css 的 `body{...background:var(--rp-bg)...}` 位于偏移 368817，main.css 的 `body{...background:#f5f7fa}` 位于 370861——后者在后；
- 运行时探针（Playwright，暗色态实测）：`body bg = rgb(245,247,250)`（亮灰）、`.el-main bg = transparent`（透出 body）、`.el-aside bg = rgb(11,18,32)`（暗，正常）；
- 截图（暗色工作台）：侧边栏/顶栏/卡片全暗，主内容区整片亮灰，视觉割裂明显。

**影响**：验收声明"明暗两套视觉完整"不成立——暗色主题主画布失效；`--rp-bg` 暗值与 `--rp-font` token 沦为死代码（token 面上 `--rp-font` 仅 components.css:8 一处使用且被覆盖）。

**最小修复**（二选一，1–3 行）：
1. 删除 `main.css` body 规则中的 `background` 与 `font-family` 两行（保留 `margin:0`）；或
2. 将 `import "./styles/main.css"` 移至 `import "./styles/tokens.css"` 之前。
修后目检暗色工作台画布（应 #0b1220）。

---

## M1（建议合并前顺手修）

### M1 暗色主按钮对比度 2.98:1（<3:1），且暗色主色偏离计划文档定值
- `frontend/src/styles/tokens.css:70`：`html.dark { --rp-primary: #818cf8 }`（Indigo-400）；Element 主按钮白字（--el-color-white）于此底上 WCAG 对比 **2.98:1**——低于大文本 3:1 阈值，更远低于 4.5:1；
- 计划文档（同 repo `docs/superpowers/plans/2026-11-16-ui-redesign.md:11`）原定 dark 主色 Indigo-500 **#6366F1**（白字对比 4.47:1）——代码取 #818cf8 既是计划-代码漂移，也引入对比缺口；
- 亮色侧无此问题：白字于 #4f46e5 = **6.29:1** ✓。
- **建议**：dark 主色回调 #6366f1（或暗色主按钮文字改 --rp-primary-light 系深色）。

---

## I1（注记，不阻断）

- **I1-1 主题按钮位置口径不一**：任务声明与台账 T1（计划文件 :48"顶栏切换按钮"）均称"顶栏"，实际唯一按钮在**侧边栏底部**（MainLayout.vue:71-86 `.sidebar-foot`，Moon/Sunny + data-test theme-dark/theme-light）；README 新增行写"侧边栏主题切换按钮"正确。功能无差，建议台账勘误。
- **I1-2 钉数口径**：声明"全量 19×2 全绿"——实际 spec 为 **20** 条（main 19 + 本增量 1），"×2"实为 retries=2（playwright.config.ts）易误读为双跑全绿；且实测为 19 过/1 败（C1-1）。建议统一为"20 钉"口径并如实记失败。
- **I1-3 未接线 token**：`--rp-accent`/`--rp-success`/`--rp-warning`/`--rp-danger`/`--rp-info`/`--rp-shadow-hover` 定义后全仓零使用（grep 仅 `--rp-font` 一处且被 C1-2 覆盖失效）——"accent 橙关键动作用"目前是纯声明，后续增量接线或删除。
- **I1-4 text-3 亮色对比偏低**：`#98a2b3` 于白底 2.58:1（brand-sub/login-sub 12px 小字，低于 4.5:1）——属设计系统常见辅助灰，注记即可。
- **I1-5 声明数值勘误**：任务声明"白字于 #4f46e5 底=对比约 4.6:1"，实测 **6.29:1**——结论方向不变（达标），数值修正入档。

---

## 验证通过项（与声明可对账部分）

| 项 | 声明 | 实测 | 结论 |
|---|---|---|---|
| 后端测试 | 174 全绿 | `TEST_DATABASE_URL=postgres://postgres:root@localhost:5432/postgres cargo test --workspace` → **174 passed / 0 failed**（45 个测试二进制） | ✓ |
| clippy / fmt | 0 / 过 | `cargo clippy --workspace --all-targets` 0 warning；`cargo fmt --check` exit 0 | ✓ |
| Vitest | 38 | `npx vitest run` → **38 passed**（含 themeFromStorage 2 钉：dark 取 dark / light+非法值+null 回退 light，unit.spec.ts:295-305） | ✓ |
| vue-tsc / build | 过 | 均 exit 0（build 既有 chunk>500kB 警告，非本增量引入） | ✓ |
| 探针 C（非 demo 库 plm_rv29 @18099） | 恰 4 行 / PUT 400 / viewer 403 | GET /sec/params 恰 4 行（checkout_hours/lock_minutes/lock_threshold/password_min_length，无 impl_checklist_done/demo_seeded）✓；PUT impl_checklist_done→400、PUT demo_seeded→400（RANGES 白名单含 checkout_hours，service 层未知键同样 400）✓；新建 viewer 账号 GET params→**403** ✓ | ✓ |
| T1 tokens/useTheme | 机制核对 | STORAGE_KEY 常量 "rustplm-theme" ✓；initTheme() 于 `app.mount` 之前（main.ts:23-25 区域，SPA 内防闪烁）✓；toggleTheme 先 localStorage.setItem 后 applyTheme（useTheme.ts:31-36）✓；导入顺序 dark/css-vars.css → tokens.css（html.dark 同特异性后导入者胜，build 产物中 token 覆盖块位于 Element dark 基座之后）✓ | ✓ |
| 前置自修① | adminOnly 显隐修复 | MainLayout.vue:56-63 NAV adminOnly 标记 + `v-if="!n.adminOnly || auth.canManageSec"`；role visibility 钉（editor/viewer 均无"用户管理"菜单，flow.spec.ts:331）实跑 **PASS** | ✓ |
| 主题钉 T3 | html.dark 切换/刷新持久/还原 | flow.spec.ts:911-934 实跑 **PASS**（初始态检测→切换断言→reload 持久→还原不污染） | ✓ |
| data-test 零破坏 | 全量既有钉 | 除 C1-1（数据性失败）外 **19/19 PASS**（global-search/看板/BOM/待办/全局搜索/预览/检出锁/关联/部门筛选等） | ✓ |
| 回扣 28 | Value 归一/Env::prefixed/seed 幂等 | main.rs:41-43 `demo_seed: serde_json::Value`、main.rs:69 `Env::prefixed("PLM_")`；demo 库实测 seed 99+admin=100 幂等标记 | ✓ |
| 回扣 23/26/27 | 修复轮完好 | set_param FOR UPDATE 事务（service.rs）；RANGES+checkout_hours PUT 白名单（探针 PUT 400 实证）；obj_link 关联钉/检出锁钉 PASS | ✓ |
| raw hex 纪律 | 无组件硬编码 | 改动文件中 raw hex 全部位于 tokens.css（定义处，正当）与品牌渐变装饰（MainLayout.vue:124 `#7c3aed`、Login.vue rgba 光斑，注记例外）+ main.css（即 C1-2 本体）；组件层无散落硬编码 | ✓（main.css 除外） |
| 可访问性静态项 | 焦点可见/reduced-motion | components.css `:focus-visible/:focus-within` outline 2px + `@media (prefers-reduced-motion: reduce)` 全局降动效 ✓ | ✓ |

---

## 复跑环境与清理记录

- 后端：TEST_DATABASE_URL 指向本机 postgres:5432；174/0。
- 探针 C：建库 plm_rv29 → server @18099（无 PLM_DEMO_SEED）→ 四项探针全过 → taskkill + DROP DATABASE plm_rv29 + 数据目录清除。
- Playwright：建库 plm_rv29_e2e → server @18095（PLM_ADMIN_PASSWORD=e2e-pass-123，PLM_DEMO_SEED=1）+ vite 5173（VITE_API_BASE 注入）→ 全量 20 钉 → taskkill + DROP DATABASE plm_rv29_e2e + 数据目录清除。仓库工作树干净（test-results 为 gitignore 产物）。

## 修复轮验收清单（转 PASS 条件）

1. C1-1：service.rs `params()` 改正向 4 键 IN（与 RANGES 一致）→ demo 库 `GET /sec/params` 恰 4 行 → security params 钉过 → 台账与提交信息按实改写（"修复实际落地"）。
2. C1-2：main.css body 去除 background/font-family（或调整 import 顺序）→ 暗色工作台画布实测 #0b1220 → 目检截图入档。
3. （建议同轮）M1：dark `--rp-primary` 回调 #6366f1。
4. 修后基线复跑：cargo test 174 / vitest 38 / Playwright 全量（demo 库）全绿，台账钉数口径改"20 钉"。

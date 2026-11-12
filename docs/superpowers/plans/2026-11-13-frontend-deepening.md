# 增量11：前端工作台深化 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 用户"一直往前做+按 superpowers 流程稳步推进"常设指令下自主决策执行；fresh reviewer 终审为质量门。

**Goal:** 工作台首页从占位变真看板（4 状态计数）；详情页 BOM 版本切换（live/发布快照 as-of）；strict 档 checked 态审批按钮（M2 债清偿）；密级可见；用户管理页（建号/授权/回收，含后端 list_users 小端点补齐）——前端闭环管理面。

**短设计（Approaches）:**
- **A（选定）纯前端消费既有 API + 一个小后端补端**：看板=按 state 过滤 4 次 list 取 total（SMB 量级 4 请求可忽略，零后端改动）；BOM 版本切换复用 versions.snapshots + bomExpand(major)；checked 按钮由 flowUi 纯函数驱动（机制既有）；用户管理需 `GET /api/v1/sec/users`（后端无此端点——小补，sec:manage 门控，JOIN 聚合角色）。
- B（否）看板走后端聚合端点 /stats：为 4 个 count 加端点+DTO+测试——量级不值，A 的 4 请求语义完全等价。
- C（否）用户管理复用 CLI：违背"前端闭环管理面"目标（PRD SMB 用户自助管理）。

**Spec:** PRD §5 工作台、M2/M8 台账债、增量9/10 交付面（versions/?major=/IM 卡片在前端的管理出口后置记账）。

## Global Constraints

- **flowUi checked**：`checked → {approve:true, reject:true, submit:false, obsolete:false, revise:false}`（strict 第二级）；其余态回归不变（Vitest 全穷举钉）。
- **看板**：工作台首页 "/" 渲染 4 张状态卡（working/review/released/obsolete 计数），点击卡片跳转 /docs 并按状态过滤（列表页加 state 筛选联动——ListParams.state 已支持）；ABAC 密级内计数（list 端点天然过滤）。
- **BOM 版本切换**：详情页 BOM 卡头 el-select：`live`（缺省）+ versions.snapshots 各 major；切换调 bomExpand(http, objNo, mode, major?)；major 无快照时后端 400 → extractApiError 提示（正常路径不触发——下拉项来自快照列表）。
- **密级**：列表加"密级"列（数字徽章色阶 0/1/2/3）；详情 descriptions 已有密级行（增量8 前已有）。
- **用户管理**：`GET /api/v1/sec/users`（sec:manage）返回 username/max_secret_level/roles[]/created_at；页面 /admin/users：建号表单（username/password/max_secret_level）+ 用户表（角色 tag + 授予/回收 select）+ 角色列表来自 GET /sec/roles；菜单项仅显示（M8 显隐债不变），viewer 点击→403 错误提示（后端门控）。
- **e2e 域**：Board 计数断言用"≥1 released"（journey 先行留下 released 件）；用户管理全程 admin（Playwright 复用 token 不切登录）。

## Review Focus

1. **flowUi checked 全穷举回归**（checked→approve+reject；working/released/obsolete/unknown 不变）→ Vitest 钉
2. **看板 4 卡计数与列表过滤一致**（released ≥1 + 点击卡片联动 /docs?state=）→ Playwright 钉
3. **BOM 版本切换：live 与 major=1 行集不同且符合后端 e2e 语义**（v1 冻结/v2 live）→ Playwright 钉（page.request 建 BOM）
4. **用户管理：建号→列表出现→授予 editor→角色 tag 更新**→ Playwright 钉；viewer GET /sec/users 403（后端 e2e 钉）
5. **list_users 仅 sec:manage（viewer/editor 403）；roles 聚合正确**→ 后端 e2e 钉

## File Structure

```
crates/core-sec/src/service.rs(+list_users)
crates/api-gateway/src/routes/sec_users.rs(+list_users 端点) + mod.rs(装配 docs_r? 否——sec:manage 组)
crates/plm-server/tests/e2e.rs(+list_users 权限钉)
frontend/src/utils/flowUi.ts(+checked)
frontend/src/api/{sec.ts(新: SecUser/Role/listUsers/createUser/grantRole/revokeRole), docs.ts(不动)}
frontend/src/pages/{Workbench.vue(看板), DocDetail.vue(BOM 版本切换), DocsList.vue(密级列), UserAdmin.vue(新)}
frontend/src/router/index.ts(+ /admin/users) + layouts/MainLayout.vue(+菜单)
frontend/tests/unit.spec.ts(flowUi checked 穷举)
frontend/tests/e2e/flow.spec.ts(+board/bom-version/user-admin 钉)
docs/superpowers/plans/2026-11-13-frontend-deepening.md；README 深化段；台账
```

## Tasks

### Task 1: flowUi checked + Vitest 穷举（M2 清偿）
- Step1 失败测试（unit.spec.ts checked 用例）；Step2 RED；Step3 实现 flowUi checked 分支（DocDetail 按钮 ui.approve/reject 自动驱动——无模板改动）；Step4 GREEN（vitest）；Step5 Commit

### Task 2: 后端 list_users + 权限钉
- Step1 失败 e2e：admin list_users 200（roles 聚合正确）+ editor/viewer 403；Step2 RED；Step3 实现（SecService::list_users + 端点 + mod.rs sec_mgmt 组装配）；Step4 GREEN；Step5 Commit

### Task 3: 看板 + 密级列 + BOM 版本切换
- Step1 实现（Workbench 4 卡+点击跳转；DocsList 密级列；DocDetail BOM major 下拉）；Step2 Vitest（flowUi 已绿）+ vue-tsc；Step3 Playwright 钉（board released≥1+点击联动；BOM 切换 live/major=1 行集不同）；Step4 Commit

### Task 4: 用户管理页
- Step1 实现（api/sec.ts + UserAdmin.vue + 路由 + 菜单）；Step2 Playwright 钉（建号→列表→授权 tag）；Step3 Commit

### Task 5: README/台账/全门禁/终审/合并
- Step1 README（工作台深化段）+ 台账（M2 清偿入账 + IM 管理出口等 deferred 复述）；Step2 全门禁（cargo test workspace + fmt + clippy + vitest + vue-tsc + Playwright 实跑）；Step3 Commit；Step4 fresh reviewer 终审→修复轮→合并 main→push（SSH 通道）→CI 绿确认

## Self-Review

覆盖看板/版本切换/checked/密级/用户管理 ↔ Review Focus 5 条 ↔ Task1-5 ✓；签名一致（list_users 走 sec_mgmt 组、flowUi 接口仅加 checked 已在增量8 预留字段）✓；范围克制（IM 死信重放/卡片按钮渲染仍 deferred 记账，不塞本增量）✓；比例（后端 1 小端点/前端 4 面）✓

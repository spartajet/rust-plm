# 增量28 终审报告——非标制造业 Demo 数据包（100 人公司+全套演示数据）

- 评审角色：独立终审 reviewer（fresh eyes）
- 分支：feat/demo-seed（基线 main @ 9a28af6）；对审 commit：f98756e / 956ec74 / 35ae9b4 / 5a3ec43（计划 061288a）
- 日期：2026-11-16
- 方法：git show 全量 diff + 代码通读（seed.rs 全文/main.rs 接线/core-sec/core-version/core-bom/mod-part/UserAdmin）+ 实跑（后端 174/clippy/fmt/Vitest 36）+ 真库探针（plm_rv28 全生命周期：首启 seed→psql/HTTP 计数→幂等重启→无 env 重启→删标记重跑→活体 openapi）+ Playwright 单钉验证

## 总判定：FIX-THEN-MERGE

两个 C1：T4 部门下拉模板从未提交（功能缺失+新钉必红+“17×2 全绿”不可对账）、fmt --check 失败。二者均为小修复，修复后可过。

---

## C1（阻断，合并前必须修）

### C1-1 UserAdmin 部门下拉框（dept-filter）模板从未提交——T4 功能缺失+Playwright 新钉必红

- 文件：`frontend/src/pages/UserAdmin.vue`（全文 1-275 行）；对照 `frontend/tests/e2e/flow.spec.ts:885-907`
- 证据：
  - script 段齐全：`DEPARTMENTS` 十部门常量（L23-26）、`deptFilter` ref（L27）、`watch(deptFilter)`（L44-47）、`listUsers(..., deptFilter.value || undefined)`（L67）。
  - **template 段（L158-275）没有任何元素绑定 deptFilter，无 `data-test="dept-filter"`**（grep 全文件仅 script 区 5 处命中，模板区 0 处）。`deptFilter` 恒为 `""`→ 传参恒 undefined → 部门筛选 UI 无入口，后端 `?department=` 从前端永不可达。
  - e2e 新钉 `flow.spec.ts:894` 要求 `await expect(page.locator('[data-test="dept-filter"]')).toBeVisible()`。
  - **实跑验证**：真实 PLM_DEMO_SEED 库（users=99）+ vite dev :5173 下跑该钉——3 次（含 retries）全部失败于 `element(s) not found`；同环境跑既有钉 `user admin: create user and grant role` **通过**（3.2s）——环境健康，失败纯因元素缺失。
- 对账结论：commit 35ae9b4 message“UserAdmin DEPARTMENTS 十部门下拉(dept-filter)+Playwright 钉——真实 PLM_DEMO_SEED 库 17×2 全绿”与代码现状不可对账：下拉从未进过 commit（git log 该文件仅此一次改动，工作树 clean），新钉在当前代码下必红，“17×2 全绿”不可能为真（至多 16+skip 或当时本地文件与提交不一致）。
- 最小修复（约 6 行）：toolbar 内补 `<el-select v-model="deptFilter" clearable placeholder="按部门筛选" data-test="dept-filter" style="width:160px"><el-option v-for="d in DEPARTMENTS" :key="d" :label="d" :value="d" /></el-select>`；补后该钉应绿（后端筛选已实测 22 行正确）。

### C1-2 cargo fmt --check 失败——基线声明“fmt 过”不可对账

- 文件：`crates/plm-server/src/main.rs:202-203`
- 证据：`cargo fmt --check` exit 1，diff 位于 demo_on 判定第二行：
  `-        || cfg.demo_seed.as_str().map(|s| s.eq_ignore_ascii_case("true")) == Some(true)` 需重排为 6 行链式。
- 最小修复：`cargo fmt`（纯格式化，无语义变化），重跑 fmt --check 与受影响 crate 测试。

---

## I1（重要，合并前应修/勘误）

### I1-1 声明“README PLM_DEMO_SEED”不成立

- 文件：`README.md`
- 证据：`rg "DEMO_SEED|demo" README.md` 零命中；本增量对 README 的唯一改动是“27 个增量→28 个增量”（diff 1 行）。部署文档的环境变量行已有（docs/部署文档.md L90）。
- 修复：README 快速开始/环境变量处补一行 PLM_DEMO_SEED（指向 demo-guide.md），或在台账勘误该子项。

### I1-2 seed 部分失败后重跑为“半幂等”：物料翻倍 + 后段死锁，声明“ON CONFLICT 幂等兜底”过强

- 文件：`crates/plm-server/src/seed.rs`（①L348-369/②L396-414/③L420-442/④L478-526/⑤L543-613/⑥L616-621/⑦L643-651）；`crates/mod-part/src/service.rs:145-178`
- 证据（真库活体：改 sj01 password_hash 为哨兵值+删 demo_seeded 标记+PLM_DEMO_SEED=1 重启）：
  - ① 用户 upsert 幂等 ✓（哨兵 hash 保留，demo-pass-123 登录 401——同时活体验证了“seed 不覆盖既有用户口令”，见 D 项）；
  - ② 图纸幂等 ✓（ObjNoDuplicate 吞，drawings 恒 36）；
  - ③ **物料翻倍**：cm_object 84→132（+48）——`PartService.create` 自动赋码（next_code）永不撞 obj_no，查重 MVP 不在 create 路径拦截，seed.rs ③ 的 `Err(DomainError::ObjNoDuplicate(_)) => {}` 分支在该路径为**死防御**；
  - ④ 随即 Err：`bom parent state=released`（set_lines 仅 working parent，core-bom/src/store.rs L100-108）→ run 失败 → 服务照常起（warn“不影响服务启动”）✓、标记未写 ✓（flag=0）——但⑤⑤ transition IllegalTransition、⑥ checkout DocLocked 同理，标记缺失时的每次重启都在 ④ 死锁，物料每次再翻倍。
- 影响：触发条件=seed 中途失败或 demo_seeded 标记丢失。demo-guide.md L49 已有“清库重建即整套重来”缓解，但台账声明“重启重跑（ON CONFLICT 幂等兜底）”只对用户/obj_link/图纸成立。
- 最小修复：文档与台账措辞改为“部分失败后须清库重建”；或 ③ 前按 `attrs->>'name'` 查已存在跳过（一段 SELECT 过滤，替代死防御分支）。

### I1-3 demo-guide.md 账号表角色列与 seed 代码不符（两处）+ 数据地图 BOM 台数点名错误

- 文件：`docs/demo-guide.md` L17-18、L32；对照 `crates/plm-server/src/seed.rs`
- 证据：
  - 生产部（L17）：“部长/班组长 editor；计划员 viewer”——代码 `DeptSpec{code:"sc", roles:&["viewer"]}`（seed.rs L115）全员 viewer；
  - 质量部（L18）：“editor / viewer”——代码 `roles:&["editor"]`（L142）全员 editor；
  - 数据地图 BOM（L32）：“3 台设备树（环形输送线/视觉分拣站/**堆垛单元**）”——代码 5 次 set_lines：ZP-01 环形输送线、BJ-101 顶升旋转单元、BJ-103 同步带传动模块、ZP-02 视觉分拣站、BJ-102 铝型材机架段（seed.rs L478-526）；**堆垛单元 DWG-FJS-ZP-03 无 BOM**。演示幕 4/幕 5 按文档操作会与实际权限/数据不符。
- 修复：两行角色列改为与代码一致（生产部全 viewer/质量部全 editor）；BOM 行改为“3 台设备（环形输送线/视觉分拣站及其部件/模块子树共 5 棵 BOM）”或把堆垛单元补一棵树。

### I1-4 “released 6”生命周期口径：终态库内 released=4（快照 7 对）

- 文件：`crates/plm-server/src/seed.rs` L553-589；`crates/core-version/src/store.rs` L129-134（revise 原地 UPDATE major+1）
- 证据（真库 psql）：`state` 分布 drawing：review 8 / released **4** / working 23 / obsolete 1；LJ-3006/3007 仅存 v2 working 行（revise 原地升版，v1 released 留痕于 cm_obj_snapshot）。快照=7 ✓（6 次 release+3010 release，revise/obsolete 不加快照——与 e2e demo_seed.rs L85-88 断言一致）。日志/SeedResult `released=6` 是**发布动作计数**，可对账；但声明“生命周期分布 review 8/released 6”若读作终态分布则不实（4+2v2 working）。
- 修复：demo-guide 数据地图行措辞澄清（“7 张发布快照（其中 2 件已升版 v2 working，终态 released 4 张+快照历史可查）”）；声明勘误。

---

## M1（次要/建议）

### M1-1 seed checkout 传 display_name 给 `username: &str` 形参

- `crates/mod-doc/src/service.rs:452-458` 形参名 `username: &str`；seed.rs L616-621 传 `uid("sj07"), "黄文涛"`（display_name）。锁横幅 by_name 显示中文姓名（演示效果反而更好），但参数语义偏移。建议：seed 侧加注释“checkout 第 3 参实为锁面展示名”，或 service 改形参名 `by_name`。

### M1-2 生产误开防护仅文档级（建议补启动 WARN）

- 现状：部署文档“生产严禁开启”+幂等标记。PLM_DEMO_SEED 开启时启动日志只有 INFO 完成行，无风险提示。建议（非阻断）：demo_on 分支入口加 `tracing::warn!("PLM_DEMO_SEED 已开启：将创建 100 个统一口令(demo-pass-123)账号——仅演示环境使用")`。

### M1-3 demo-guide“seed 耗时约 3-5 秒”实测 1.2s（audit elapsed_ms=1167）

- 低危夸大，无碍；可顺手改“约 1-2 秒”。

---

## 验证通过项（声明与代码/实跑可对账）

| 项 | 证据 |
|---|---|
| T1 迁移 0021 | `crates/core-sec/migrations/0021_user_department.sql`（ADD COLUMN IF NOT EXISTS+索引，幂等，只增不改）；真库 `information_schema.columns` department text 存在；`_sqlx_migrations` version=21 已录；docs/migrations.md 已注册 |
| T1 list_users | `crates/core-sec/src/service.rs` SQL 加 u.department 列+`$4` 精确筛选（params 顺序正确：总查询 4 参/计数 2 参分开绑定）；`crates/api-gateway/src/routes/sec_users.rs` UsersQuery.department+透传；真库 HTTP 实测 `?department=机械设计部` total=22、items[0]=sj01/机械设计部/['editor']/max 2 |
| T2 人员 100 | 真库 sec_user=100、department<>'' 覆盖 99、十部门人数逐一相符（总经办2/机械22/电气12/工艺8/生产18/质量10/采购6/仓储5/销售9/财务7）；DEPTS names 长度≥count 全部门核对通过（dq 两个重名“袁变频/袁线束”仅 display_name 无碍） |
| T2 统一口令 | `core_sec::hash_password_public` 一次散列复用（core-sec/src/service.rs L646-649）；sj01 HTTP 登录 200；e2e 登录钉在 174 全绿内 |
| T2 upsert 不覆盖口令 | seed.rs L351 `ON CONFLICT (lower(username)) DO UPDATE SET department=..., display_name=...`（无 password_hash）——代码确认+活体验证（哨兵 hash 保留+demo-pass 401） |
| T2 图纸 36 | 显式三级编号（ZP3/BJ11/LJ22=36）；stage_file→upload_staged 真实落盘；创建人 designers=sj05..sj12 轮换；真库 drawing=36 |
| T2 物料 48 | ZZ28+WG20=48 自动赋码；采购/仓储 buyers 轮换；真库 part=48 |
| T2 BOM | 5 次 set_lines（zp1→bj101→bj103 深度 3 链真实闭包）；e2e expand≥8+跨级外购件断言在 174 全绿内；真库闭包经 e2e 覆盖 |
| T2 快照/锁/关联 | 真库 cm_obj_snapshot=7、doc_lock=2、obj_link=12（from 均指向 deleted_at IS NULL 行，0 条悬挂）；audit_log 含 admin/demo.seed 完整计数行 |
| T3 接线 | main.rs L44-45 Value 承载+L64 Null 默认+L204-207 三态归一（"1"/true/1/bool）；Env::prefixed("PLM_") 在（L69）；PLM_DEMO_SEED=1 实测触发；**不设实测零 demo 日志+计数不变**；seed Err 实测 warn 后服务照常起 |
| T4 部分 | 部门列（L174-176）✓、DEPARTMENTS 十部门字面量与 seed DEPTS 名称逐字一致（含顺序）✓、watch 立即筛选+page 重置 ✓、listUsers department 传参 ✓——下拉框缺失见 C1-1 |
| T5 部分 | demo-guide.md（账号表样例账号 zb01 李建国…cw01 王财务与 DEPTS names 逐一相符/数据地图/五幕/调试要点）✓；部署文档环境变量行 ✓；migrations.md 0021 注册+四裁定 ✓；计划台账 ✓——README 缺失见 I1-1、角色/BOM 不符见 I1-3 |
| 后端测试 | cargo test --workspace（target-rv28）45 个 summary 行合计 **174 passed 0 failed** |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` exit 0 |
| Vitest | 36/36 passed |
| openapi 活体 | GET /api/docs/openapi.json 按 e2e 同款 extract_face 逻辑对比快照：33 paths/全部 schema **无 breaking 无 stale，MATCH** |
| 幂等 | PLM_DEMO_SEED=1 二次重启日志“Demo 数据包已存在（demo_seeded 标记）——跳过”+计数 100/84/7/2/12 不变 |
| e2e 隔离 | demo_seed.rs 用 TestCtx（plt_dseed_*临时库）不污染 e2e 库；plm-server 不设 env 时 main 零副作用（实测） |
| 回扣增量27 | duplicates helper 在（api-gateway/src/routes）；sqlx migrate! ignore_missing 模式未动；0021 未 touch（该文件 diff 仅 5 行 SQL） |

## Playwright 说明

17 个测试=16 既有+1 新钉。新钉在当前代码必红（C1-1，实测 3 连败）；同环境既有钉对照通过证明环境健康。**“17×2 全绿”声明按当前提交不可对账**——补 C1-1 模板后应重跑全量钉确认。

## 修复清单（按序）

1. UserAdmin.vue template 补 dept-filter 下拉（C1-1，~6 行）
2. `cargo fmt`（C1-2）
3. 重跑：fmt --check、cargo test -p plm-server、Playwright 全量 17×2（真实 demo 库）
4. README 补 PLM_DEMO_SEED 行或声明勘误（I1-1）
5. demo-guide 角色/BOM/快照措辞勘误（I1-3/I1-4）+台账“幂等兜底”措辞收敛（I1-2）
6. （可选）M1-2 启动 WARN、M1-1 注释、M1-3 耗时勘误

## 探针环境清理确认

plm-server 进程已终止；vite 进程已终止；5173/18095/18099 端口无监听；plm_rv28 与 plm_rv28_e2e 已 DROP；$TEMP 数据目录已删。

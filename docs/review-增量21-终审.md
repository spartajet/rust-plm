# 增量21 终审报告（BOM-01 编辑 UI + BOM-07 汇总报表）

- 审阅人：独立终审 reviewer（fresh eyes，未参与开发）
- 分支：`feat/bom-editor-summary`（基线 main @ 411108d，增量20 合并后）
- 待审 commit：c23c6aa（T1）/ a55eb14 + adcdc8b（T2）/ e053ad7（T3）/ 9c9cc22（T4）
- 审阅方式：git show 四 commit 对账 + 代码精读 + 本机复跑全量测试 + 真库（plm_rv21）API 探针 + OpenAPI 活体对账

---

## 一、总判定

**PASS**（C1 = 0；I1×2 / M1×5 建议修复，不阻塞合并）

声明与代码可对账，红线未破。全部基线指标实证复现：后端 **165 passed / 0 failed**、clippy **0**、`cargo fmt --check` 过、前端 Vitest **32/32**、Playwright flow.spec.ts **13** 个 test 块（两轮全绿为开发侧声明，本次静态计数核对，未复跑——不在审阅范围 B 内）。真库探针 12 项全过，增量20 终审 I1-1 修复（search DB 密级权威复核）回扣完整。

---

## 二、逐项对账（声明 → 证据）

### T1（c23c6aa）core-search obj_id ✅

| 声明 | 证据 |
|---|---|
| SearchHit.obj_id（uuid STORED） | `crates/core-search/src/service.rs:25`（SearchHit 字段）、`:63`（`add_text_field("obj_id", STORED)`）、`:117`（ScanRow.id + SCAN SQL 增查 `o.id`）、`:183`（索引写入）、`:393-397`（hit_of 回读） |
| 索引目录 search→search-v2 一次性废弃重建 | `crates/plm-server/src/main.rs:186-187`、`crates/plm-server/tests/common/mod.rs:137`（两处 open 目录均 v2，含 degraded_app `:371-374`） |
| uuid 依赖 | `crates/core-search/Cargo.toml:14` + Cargo.lock |
| crate 测试含 obj_id 断言 | `crates/core-search/tests/search_it.rs:98-101`（`!obj_id.is_empty()` 钉） |

注：v2 为"废弃重建"策略——启动即全量重建（真库探针实证：新库启动日志 `搜索索引全量重建完成`），旧目录残留无害，migrations.md 已记裁定。

### T2（a55eb14/adcdc8b）core-bom + 路由 ✅

| 声明 | 证据 |
|---|---|
| lines_info：live=line_no 序 | `crates/core-bom/src/store.rs:247-259`（`ORDER BY e.line_no`，SQL 内 `secret_level <= $2` + `deleted_at IS NULL` 过滤越密级子件） |
| lines_info：快照=child 聚合 line_no=0 占位 | `store.rs:261-287`（`SELECT 0 AS line_no ... GROUP BY e.child ... ORDER BY o.obj_no`） |
| bom_release 存在性校验无快照 400 | `store.rs:267-274` / `store.rs:336-342`（`DomainError::BadRequest("major {N} 无发布快照")` → 400） |
| summary：live=bom_path leaf 多路径 qty_path 和 | `store.rs:293-309`（`SUM(p.qty_path)` + `NOT EXISTS(bom_edge 子边)` 叶过滤） |
| summary：快照=递归 CTE | `store.rs:311-351`（锚=直接子，递归 `t.qty * e.qty`，叶过滤 `NOT EXISTS(bom_edge_snapshot)`——与 expand_asof LeafOnly 同构） |
| 子件密级 ABAC SQL 内过滤 | 四条 SQL 均含 `o.secret_level <= $N`（越级行不出 SQL，不泄露存在性） |
| 路由 GET lines / summary（JSON 或 ?format=csv） | `crates/api-gateway/src/routes/boms.rs:256-360`；CSV：UTF-8 BOM `\u{FEFF}` + `text/csv; charset=utf-8` + `Content-Disposition: attachment; filename="summary-{obj_no}.csv"` + 引号转义；路由注册 `routes/mod.rs:76-78`（docs:read 层） |
| OpenAPI 注册 | `crates/api-gateway/src/state.rs:83-84`（paths）+ `:97-98`（schemas）；活体 openapi.json 对账见 §四-D |
| e2e `bom_lines_info_summary_csv_and_abac` | `crates/plm-server/tests/e2e.rs:2834-3061`（富行 3 行 line_no 序 / 叶集恰 2 / WG 多路径和=8 直挂3+经C 5×1 / 越密级叶剔除不泄露 / CSV BOM 头 / 发布→revise→清 live→major=1 快照仍在 / major=9 → 400） |

### T3（e053ad7）前端 ✅（1 处台账归属误差见 I1-2）

| 声明 | 证据 |
|---|---|
| 富行展示（编号/名称替代裸 uuid） | `frontend/src/pages/DocDetail.vue` 展示态表格 `child_obj_no`/`child_name` 列；快照行 `line_no=0` 显示 "-" |
| 编辑态（qty 行内/删行/搜索选件取 obj_id/保存全量 set_lines 409 透出） | 同文件 `startEdit/removeRow/searchForAdd/pickHit/saveEdit`；`pickHit` 拒空 obj_id、拒自引用；`saveEdit` 错误经 `extractApiError` 原样透出（409/400） |
| as-of 只读 | `DocDetail.vue:105` `bomEditable = auth.canWrite && bomMajor===0`（仅 live 可编辑）；watch 切 major 强制 `editing=false` + `editRows=[]`（竞态清退 ✓） |
| BOM-07 汇总折叠懒加载 + CSV blob 下载 | el-collapse `@change` 触发 `loadSummary()`；`exportCsv` blob + `a.download="summary-{objNo}.csv"` |
| api 五函数 + 3 单测 | `frontend/src/api/docs.ts:180-256`（bomLines/setBomLines/bomSummary/downloadBomSummaryCsv/buildSetLinesPayload）；`frontend/tests/unit.spec.ts:246-265` 恰 3 个 it |
| 回归修复① watch(bomMajor) 丢 reloadBom | `DocDetail.vue:199-206`：`editing=false; editRows=[]; bomSummaryLoaded=false; bomSummaryRows=[]; reloadBom()`（原增量11 语义保留 ✓） |
| 回归修复② 网关 SearchHitDto 漏 obj_id | `crates/api-gateway/src/routes/search.rs:27-28` + From 转换 `:41`；链路完整：core-search SearchHit → DTO → `frontend/src/api/search.ts:7-8`（`obj_id?: string`） |
| **parts 测试查重确认弹窗条件处理** | **归属误差**：该处理系增量20 修复轮 9910594 引入（`frontend/tests/e2e/flow.spec.ts:41-42` 注释自证"增量20"）；e053ad7 对 flow.spec.ts 纯新增 +99 行、对 main 零删除。功能在库、声明不实——见 I1-2 |

### T4（9c9cc22）文档 ✅

README（富行/汇总/CSV curl 组 + 21 增量）、部署文档端点表新增 1 行、评审报告 BOM-01 🟡 增补编辑器 / BOM-07 ⬜→🟡、migrations.md 五条裁定（BOM 路由父件限 drawing / 行级属性扩展后置 / 成本估算后置 / search→search-v2 / 汇总 total 语义）、计划台账回填。均与代码一致。

---

## 三、测试实证（本机复跑）

| 项 | 结果 | 命令/方式 |
|---|---|---|
| 后端全量 | **165 passed / 0 failed**（42 条 `test result: ok` 行汇总，EXIT=0） | `TEST_DATABASE_URL=… CARGO_TARGET_DIR=target-rv21 cargo test --workspace`（隔离 target 避 AV 锁） |
| clippy | **0 warning / 0 error** | `cargo clippy --workspace --all-targets` |
| fmt | **通过** | `cargo fmt --check` |
| 前端单测 | **32/32** | `npx vitest run` |
| 前端类型 | 通过（vue-tsc -b EXIT=0，旁证 obj_id 链路类型闭合） | `npx vue-tsc -b` |
| Playwright | 13 个 test 块（静态计数）；两轮全绿未复跑（不在范围 B） | `grep -c "^test("` |

## 四、真库探针（plm_rv21 @ :18097，全过）

环境：建库 plm_rv21 → 起服务（PLM_HTTP_PORT=18097 / PLM_ADMIN_PASSWORD=rv21-pass / PLM_DATA_DIR=%TEMP%/plm-rv21-data）→ admin 登录 → 上传 P/C1/C2 图纸 + 建 CLS-ZZ/CLS-WG 物料。

| # | 探针 | 结果 |
|---|---|---|
| 1 | set_lines P:{C1×2,C2×3} → C1:{WG×5} | 204 / 204；qty=0 → **400**（`qty 必须为正数`，后端兜底 ✓） |
| 2 | 重构 P:{ZZ×2, WG×3, C1×1} | 204 |
| 3 | GET /boms/P/lines | **3 行**，line_no 序，child_obj_no/child_name/qty 齐（富行非裸 uuid） |
| 4 | GET /boms/P/summary | 恰 2 叶：**ZZ(2) / WG(3+1×5=8)**；C1 非叶不入；class_code/class_name 回填 |
| 5 | WG 提密 2 → 低上限用户（viewer/editor max0）lines | WG 行剔除（行 1/3 保留），不泄露存在性 |
| 6 | 低上限用户 summary | 仅剩 ZZ；越密级叶剔除 |
| 7 | ?format=csv（admin） | `\u{FEFF}` BOM（od 实证 357 273 277）+ 表头 `编号,名称,分类码,分类,单台用量` + Content-Type/Disposition 正确 |
| 8 | ?format=csv（低上限用户） | 仅 ZZ 行——CSV 与 JSON 同源 ABAC ✓ |
| 9 | P 提密 2 → 低上限用户 lines/summary/csv | 三者均 **404**（`docs:DWG-RV21-P` 不泄露存在性） |
| 10 | 发布(approve) → revise → 清空 live | live lines=[]；**?major=1 快照富行 3 行（line_no=0 占位）/ 快照汇总 ZZ(2)/WG(8)** |
| 11 | ?major=9 | lines/summary 均 **400** `major 9 无发布快照` |
| 12 | GET /api/docs/openapi.json | `/api/v1/boms/{obj_no}/lines`+`/summary` 两 path 在、`LineInfoDto`(5 属性)+`SummaryDto`(5 属性) 两 schema 在、summary params=[obj_no,major,format]、lines 200 body=`Vec<LineInfoDto>` $ref 正确 |

清理：taskkill plm-server ✓、DROP DATABASE plm_rv21 ✓、临时文件清除 ✓。

## 五、历史易犯错误专项

| 项 | 结论 |
|---|---|
| OpenAPI 双 path + 双 schema | ✓ 代码注册 + 活体对账双证（§四-12）。旁注：`set_lines/expand/where-used` 三个既有端点有 `#[utoipa::path]` 但从未列入 paths（**先于本增量存在**，非本增量引入）——M1-5 记录 |
| ABAC：lines/summary SQL 内过滤 | ✓ 四条 SQL 均带 `secret_level <= $N`；探针 5/6/9 实证（行级剔除 + 父件越级 404 + 无存在性泄露） |
| CSV 同源过滤 | ✓ 探针 8（低上限 CSV 仅剩可见行） |
| Dockerfile | ✓ 无需改：Dockerfile 已 COPY core-search/Cargo.toml（增量20 已含），本增量仅该文件内容加一行 uuid 依赖；diff 无 Dockerfile 改动 |
| 迁移注册表 | ✓ `git diff main..feat --name-only | grep migrations` 仅 `docs/migrations.md`；无新迁移文件，五条裁定与代码一致 |
| **增量20 I1-1 回扣** | ✓ `recheck_db_secret`（`core-search/src/service.rs:435`）在 search_groups（`:495`）与 search_page（`:537`）两路径均完好；带外提密 fail-closed 语义未动 |
| SearchHitDto.obj_id 链路 | ✓ core-search（STORED+ScanRow+hit_of）→ 网关 DTO（search.rs:27）→ 前端接口（search.ts:7）→ 编辑器 `pickHit` 取 `obj_id ?? ""` 拒空；vue-tsc 闭合 |
| buildSetLinesPayload qty 兜底语义 | ✓ `qty ?? 1` 仅为纯函数兜底；UI `saveEdit` 前置拦截 null/≤0（warning 不发请求）；后端 `set_lines` `qty<=0 → 400`（store.rs:75-77）——三层语义自洽，单测注明"UI 校验兜底前" |
| 编辑器竞态 | ✓ watch(bomMajor) 内 `editing=false`+清 editRows+汇总缓存作废+reloadBom；`bomEditable` 仅 live（bomMajor===0） |
| CSV 公式注入 | 见 M1-2（判定：建议级） |

---

## 六、发现清单

### C1（必须修）

无。

### I1（应该修）

- **I1-1 死 UI 分支——"发布快照只读"提示永不渲染**
  `frontend/src/pages/DocDetail.vue:338`：`<span v-if="bomMajor !== 0">发布快照只读</span>` 位于 `<div v-if="bomEditable">`（`:325`）之内，而 `bomEditable`（`:105`）= `auth.canWrite && bomMajor === 0`——内层条件恒假，提示永远不出现（用户切到 as-of major 时得不到只读提示）。
  **最小修复**：把该 span 移出 `bomEditable` 容器（或改为 `v-if="bomMajor !== 0"` 的独立行放表格区上方）。

- **I1-2 对账红线：台账把增量20 的改动记到 T3 名下**
  `docs/superpowers/plans/2026-11-15-bom-editor-summary.md` 对账台账"T3 完成（e053ad7）"条目含"parts 测试查重确认弹窗条件处理"；待审声明同样表述。实证：该处理由增量20 修复轮 **9910594** 引入（`frontend/tests/e2e/flow.spec.ts:41-42` 注释自证"增量20"），e053ad7 对该文件纯新增 +99、对 main 零删除——代码在库但**归属不实**。
  **最小修复**：台账该句改为"parts 查重确认弹窗条件处理已于增量20 修复轮（9910594）落地，本增量仅沿用于 Playwright 13 计数"或直接从 T3 条目移除。

### M1（建议）

- **M1-1 e2e 陈旧注释与断言矛盾**
  `crates/plm-server/tests/e2e.rs:2951`：`// ② 汇总：多路径用量（外购=直挂3 + 经自制 5×2=10 → 13）` 与同函数断言（`qty_total == 8`，"外购=直挂3+经图纸C 5×1"）矛盾——系草稿结构（13）废弃后遗留，断言与 commit message"13→8 语义"均正确，仅注释误导后读者。**修复**：注释改为"外购=直挂3+经图纸C 5×1=8"。

- **M1-2 CSV 公式注入未防护（建议级，可后置）**
  `crates/api-gateway/src/routes/boms.rs` summary CSV 分支：`esc` 仅做引号转义，`name/class_code/class_name` 来自用户 attrs，首字符为 `= + - @`（或 TAB/CR）的单元格在 Excel 打开时会被当公式执行。**判定**：本系统为内网受控用户、字段为工程属性、无外部不可信输入源，风险低；CSV 定位内部分析用途。**若修**：写单元格前对以 `= + - @ \t \r` 开头的值加 `'` 前缀（或前导空格）即可，两行改动。

- **M1-3 汇总面板缓存作废后"开着的折叠"不自动刷新**
  `DocDetail.vue` watch(bomMajor) 清空 `bomSummaryRows`，但懒加载仅在 el-collapse `@change`（用户收起/再展开）时触发——major 切换时若汇总面板处于展开态，将显示"无叶子件"（误导性空态）直至用户重新折叠展开。**建议**：watch 中若面板展开则直接 `loadSummary()`，或空态文案区分"未加载/无数据"。

- **M1-4 obj_id 字段用 `STORED`（默认 TextOptions）而非 `STRING | STORED`**
  `crates/core-search/src/service.rs:63`：uuid 走默认分词器，每文档产生 ~5 个无意义 token（索引膨胀 + 位置表噪声）；功能无害（从不作为查询字段，stored 值完整回读），与 `type_code/obj_no`（STRING|STORED）风格不一致。**建议**：不为此单独再 bump search-v3（版本化策略成本>收益）；下次 schema 变更顺带改为 `STRING | STORED`。

- **M1-5 既有端点未注册 OpenAPI（先于本增量，记录备考）**
  `crates/api-gateway/src/state.rs` paths 列表缺 `boms::set_lines / boms::expand / boms::where_used`（三者的 `#[utoipa::path]` 属性齐备）。本增量两条新端点注册完整，无回归；建议后续增量把三个老端点一并补入。

---

## 七、结论

增量21 四个 commit 声明与代码、测试、活体系统三方对账一致；两处开发中回归（watch(bomMajor) 丢 reloadBom、网关 SearchHitDto 漏 obj_id）确实被既有钉拦下并已修复且留钉；增量20 I1-1 修复未被本增量削弱。发现项无 C1：I1-1 为一行 UI 位置修正，I1-2 为台账文字归属修正，均可随下个提交顺带清偿。

**总判定：PASS**（I1/M1 建议随下次提交清偿，不阻塞合并）。

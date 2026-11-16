# 增量27（DOC-12 对象关联：物料↔文档双向追溯）终审报告

- 评审人：独立终审 reviewer（fresh eyes）
- 分支：`feat/doc12-links`（基线 main @ dfc54a9；增量 c5f3f43 计划 + 2dab1e0 T1 + b1d2da4 T2 + f9a9ca9 T3）
- 日期：2026-11-16
- 结论：**FIX-THEN-MERGE**（C1×2 / I1×2 / M×4）

## 0. 验证手段（全部真跑）

| 手段 | 结果 |
|---|---|
| `git show` 四 commit + 全量代码审读（links.rs 305 行全文 / 0020_obj_link.sql / routes/mod.rs 挂载组 / state.rs OpenAPI 注册 / error.rs / DocDetail.vue / api/links.ts / flow.spec.ts / core-version store.rs 升版语义） | 完成 |
| `cargo test --workspace`（CARGO_TARGET_DIR=target-rv27，TEST_DATABASE_URL 真库） | **173 passed / 0 failed** ✓（对账：main=172 个测试属性 + 本增量新增 1 个 e2e = 173，与声明一致；TQ-02 `openapi_snapshot_gate` 含内 ✓） |
| `cargo clippy --workspace --all-targets` | 0 警告 ✓ |
| `cargo fmt --check` | 通过 ✓ |
| `vitest run` | **36/36 passed** ✓ |
| Playwright（fresh 库 + plm-server 18095（PLM_ADMIN_PASSWORD=e2e-pass-123）+ vite 5173（VITE_API_BASE 注入），两轮独立复跑） | **两轮均非全绿**：R1=15 passed+1 flaky（parts）+**2 failed**；R2=16 passed+**2 failed**（DOC-12 新钉 + SYS-04 存量钉，均确定性复现，见 C1-2/I1-1）——「17×2 全绿」声明不可复现 |
| 真库探针（plm_rv27 专库，18099 端口；跑毕 taskkill + DROP DATABASE + 数据目录清理） | 见 §2，18 项探针全过（唯 C1-1 属前端层，探针后端全对） |
| 快照基线独立比对 | 按门禁测试同款归一化逻辑（paths→methods / schemas→required+properties）对活体 `/api/docs/openapi.json` 与提交的 `openapi-snapshot.json` 做结构 diff：**完全一致** ✓ |

## 1. C1（合并阻断：功能损坏 / 声明失实）

### C1-1 DocDetail「关联物料」搜索确定性空结果——`res.data.items` 读裸数组恒 undefined，UI 关联链路不可用

- **代码**：`frontend/src/pages/DocDetail.vue:148`：
  ```ts
  const res = await http.get("/api/v1/parts/duplicates", { params: { name: q } });
  linkHits.value = (res.data.items ?? []).map((i: { obj_no: string; attrs?: { name?: string } }) => ({
  ```
  后端 `GET /api/v1/parts/duplicates` 返回**裸 JSON 数组**（`crates/api-gateway/src/routes/parts.rs:126` `Ok(Json(json!(out)))`；活体 curl 实证 `[{"material":...,"name":...,"obj_no":...}]`）——`res.data.items` 恒为 `undefined` → `?? []` → 对话框表格永远「暂无数据」。且 L150 `i.attrs?.name` 还有第二重 shape 错位（后端 `Duplicate` 序列化为 `{obj_no,name,material}`，无 `attrs` 字段）——即使第一重修好，名称列仍恒「-」。
- **影响**：DocDetail 关联物料搜索是 UI 侧唯一建关联入口——**物料↔文档关联功能从前端完全不可用**（后端四端点本身全部正常，见 §2）。
- **实证**：Playwright 两轮复跑，DOC-12 用例（flow.spec.ts:831）3 次执行（含 retry）全部在同一断言死：`link-results` 表空、`暂无数据` 快照留存于 error-context。这正是开发台账里「parts 单复跑过——并行时序轮换 flake 家族」被误判的真相：不是 flake，是确定性 bug。
- **定性**：既有 `frontend/src/api/part.ts:51` `duplicates()` helper 本就正确返回裸数组且被 Parts.vue 使用——新代码绕开既有 helper 手写请求且未对 shape 回读验证，与增量26 C1-2「声明前未回读」同族事故。
- **最小修复**：`searchPartsForLink` 改用既有 helper：`linkHits.value = await duplicates(http, q);`（`Duplicate{name}` 与模板 `row.name ?? "-"` 直接兼容，L150 的 attrs 映射一并删除）；修后重跑 Playwright 全绿，并回填声明。

### C1-2 「Playwright 17×2 全绿」声明失实——独立两轮复跑均 2 failed（C1-1 确定性失败 + SYS-04 存量矛盾钉）

- **声明**（b1d2da4 提交信息 + 计划台账）：「Playwright 钉（关联→行可见→解除→空态）17×2 全绿」。
- **真跑实证**（fresh 库、正确接线 VITE_API_BASE/E2E_API_BASE/E2E_ADMIN_PASSWORD）：
  - R1：15 passed + 1 flaky + **2 failed**；R2：16 passed + **2 failed**。两轮失败集合相同：
    1. `flow.spec.ts:831` doc links（本增量新钉，C1-1 根因，确定性死）；
    2. `flow.spec.ts:733` security params SYS-04（**存量矛盾**，I1-1，自增量26 修复轮 dd54a5d 引入即死——非本增量造成，但使「全绿」在含此钉的套件上不可能成立）。
- **定性**：声明-代码对账红线击穿（第二次连续增量发生「Playwright 全绿」声明失实——增量26 终审 C1-3 同族）。新钉本身在开发环境从未真实绿过（若绿过则 C1-1 不可能存在）。
- **最小修复**：修 C1-1 + I1-1 后重跑 17×2 全绿，台账勘误（含增量26 SYS-04 钉引入即死的说明）。

## 2. 真库探针记录（plm_rv27 @ 18099——后端行为全部正确）

| # | 探针 | 结果 |
|---|---|---|
| 1 | 迁移链 `_sqlx_migrations` | 20 条含 `0020 obj link` ✓；`obj_link` 三索引（pkey/from_idx/to_idx）+ `obj_link_from_id_to_id_kind_key` UNIQUE ✓ |
| 2 | admin 登录 + 建低上限 editor/viewer + 传图纸 DWG-RV27-1 + 建物料 WL-ZZ-00001 | 全 201 ✓ |
| 3 | POST /links（doc_part） | **201** `{"id":...}` ✓ |
| 4 | 重复 POST | **409 `MDL-LINK-DUPLICATE`** ✓（报文含 code/trace_id） |
| 5 | GET /docs/{no}/links（文档侧） | 200 items=[物料,含 class_name"自制件"] ✓ |
| 6 | GET /parts/{no}/links（物料侧） | 200 items=[文档] ✓ 双向一致 |
| 7 | DELETE /links/{id} | **204** ✓；重复 DELETE → **404 `MDL-OBJ-NOT-FOUND`** ✓ |
| 8 | viewer POST /links | **403**（docs:write 门控先于业务）✓ |
| 9 | 自关联（from=to 同号） | **400 `MDL-BAD-REQUEST`**（「from/to 不得为同一对象」）✓ |
| 10 | kind="bom_part"（白名单外） | **400**（「kind 须为 doc_part」）✓ |
| 11 | 物料 SQL 提密 2 → 低上限 editor POST 关联 | **404**（to 端越级不泄露）✓ |
| 12 | admin POST 关联越密级物料 | **201**（admin 上限内可写）✓ |
| 13 | 低上限 GET /docs/{no}/links | **200 items=[]**（越密级行级过滤）✓；admin 同查询见 1 行 ✓ |
| 14 | 低上限 GET /parts/{secret2}/links（锚端越级） | **404** ✓ |
| 15 | 低上限 DELETE 越密级关联（admin 建的那条） | **404**（解除双端预检生效）✓ |
| 16 | audit_log | `doc.link`×2 + `doc.unlink`×1，object=from_no、detail={to,kind} ✓ |
| 17 | 活体 openapi.json | 四端点+方法+CreateLinkReq/LinkItem schema 全在 ✓；与快照基线结构 diff **一致** ✓ |
| 18 | 清理 | taskkill plm-server + DROP DATABASE plm_rv27 + 数据目录删除 ✓ |

## 3. I1（合并前应修）

### I1-1 SYS-04 既有 Playwright 钉断言与前端标签矛盾（存量 main 缺陷，本增量声明依赖其转绿）

- `frontend/tests/e2e/flow.spec.ts:743`：`has-text("检出时长（分钟）")`，而 `frontend/src/api/sec.ts:91` PARAM_LABEL 为 `checkout_hours: "检出时长（小时）"`——**dd54a5d（增量26 修复轮）引入即矛盾**，git 全历史 `检出时长（分钟）` 无一处与 label 同期成立。两轮 Playwright 该钉确定性死（fail 快照：表渲染 4 行正常、唯文本断言死）。
- 定性：非本增量引入（后端 173 全绿含 checkout_hours 全部钉，本增量未触碰该行），但本增量「17×2 全绿」声明以全套件绿为前提，且该钉是增量26 终审 C1-3 亲自要求加的「防静默漂移」钉——它自己漂移了。
- **最小修复**：flow.spec.ts:743 改 `has-text("检出时长（小时）")`（或改定位 `param-input-checkout_hours` 行），一行改动。

### I1-2 「README 关联 curl 组」声明失实——f9a9ca9 只改了增量计数一行

- **声明**（f9a9ca9 提交信息 + 计划台账）：「README 关联 curl 组+27 增量」。
- **事实**：`git show f9a9ca9 -- README.md` 仅 L7「26 个增量→27 个增量」一行；README 全文 grep 无 `/links`、无关联 curl（部署文档端点表 ✓、评审报告 DOC-12 行 ✓、migrations.md 0020+四裁定 ✓ 均属实）。
- 定性：对账红线（文档类，影响小但性质同 C1-2——声明先于落点核实）。且评审报告 L70「DocDetail 关联卡 ✅」在 C1-1 修复前不成立，修复轮应一并回读修正表述。
- **最小修复**：README 快速上手区补 3 行关联 curl（POST /links、GET /docs/{no}/links、DELETE /links/{id}）或台账勘误声明；C1-1 修复后再回读评审报告 L70 表述。

## 4. M（记录在案，可随后续增量）

### M1 e2e 钉覆盖缺口：解除双端预检与物料侧反查行级过滤无钉

`doc_part_links_bidirectional_and_abac`（e2e.rs:4163）钉了建侧越级 404、文档侧反查过滤、重复/自关联/kind/audit/重复解除，但 ④⑤ 两步未钉：低权限 DELETE 越密级关联 → 404（代码 links.rs:170-182 双端预检正确、探针 #15 活体实证）、低权限 GET /parts/{no}/links 反查过滤（links_for 与文档侧共路径、探针 #14 实证锚端 404）。代码对、声明真，唯钉不全。**最小修复**：④ 补 low DELETE admin 所建越级关联 404 断言；⑤ 前补 low GET 物料侧 links 不含越密级文档断言。

### M2 create_link 重复预检+INSERT 非原子——并发下 409 降级 500

links.rs:106-123 先 SELECT 预检再 INSERT（L124-133），并发同请求双双过预检后，输家触发 UNIQUE 约束（0020 保底，完整性无损）→ `DomainError::Db` → 500 而非 409。**最小修复**：INSERT 错误分支捕获 Postgres `23505` 映射 `LinkDuplicate`。

### M3 DOC-12 Playwright 钉位弱评估（建议级）

flow.spec.ts:865 `[data-test="link-pick-${PNAME}"]` 用物料名拼 obj_no 注定 miss（obj_no 为服务端 `WL-ZZ-` 编码），`.catch(() => {})` 吞掉后退化为行文本+button 定位——能跑但弱（ Element 文本耦合）。**最小修复**：setup 回读物料 obj_no 后按 `[data-test="link-pick-${obj_no}"]` 精确钉（该 data-test 已存在，DocDetail.vue 模板 `:data-test="`link-pick-${row.obj_no}`"`）。

### M4 links.rs 死代码/误导注释

links.rs:84-88 `if secret > 0` 分支返回值与 fallthrough 完全相同（no-op），注释「此处返回 secret 由外层判」描述的行为不需要该 if；`links_for`（L209-217）第四参数 `_other_type` 未使用。clippy 零警告（下划线前缀豁免）但属误导性死代码。**最小修复**：删 if 直返；删 `_other_type` 参数。

## 5. 专项核对（全部通过）

- **权限挂载组（开发事故复核）**：routes/mod.rs 现状——POST /api/v1/links + DELETE /api/v1/links/{id} 落 `docs_rw`（L37-41，层= `require_perm("docs","write")` L43-46）；GET /api/v1/docs/{obj_no}/links + GET /api/v1/parts/{obj_no}/links 落 `docs_r`（L56-57，层= `require_perm("docs","read")` L67-70）。台账所记「python replace 静默 no-op」事故已由 Edit 直改修复，无残留。
- **ABAC 统一 404 语义**：建侧 to 越级 404（探针 #11）、锚端越级 404（#14）、解除双端预检 404（#15）——报文统一 ObjNotFound，无存在性泄露；admin 上限内可写越密级对象+读取侧行级过滤（#12/#13）与增量17「docs:read 回填真实密级」既有模式一致。
- **major 维度/升版保留推理**：`resolve` 与 `links_for` 均按 `obj_no+type` 取 `deleted_at IS NULL` 最新活版本（`ORDER BY major_ver DESC, minor_ver DESC, iteration DESC LIMIT 1`）；core-version 升版是**同 row UPDATE**（`crates/core-version/src/store.rs:129-134`，Revise 分支 `SET major_ver = major_ver + 1 ... WHERE id = $1`，id 不变）——obj_link 锚定 id，升版后 from_id/to_id 仍指向同一行且仍是该 obj_no 最新活版本，**关联天然保留、双向查询不丢**。推理成立（快照/历史版本粒度的关联语义未来如需区分，随 kind 域扩展裁定）。
- **audit**：doc.link / doc.unlink 两 action 落 audit_log（异步 spawn 不阻塞主链路），object=from_no、detail 带 to+kind——探针 #16 与 e2e ⑥（e2e.rs:4381-4395，含 2s 轮询容忍）双验证。
- **快照基线**：独立归一化 diff（非复用测试代码）= 完全一致；TQ-02 门禁 `openapi_snapshot_gate` 在 173 绿内；本增量 diff 36 行恰为四路径方法面+CreateLinkReq/LinkItem 两 schema 面，无夹带。
- **增量26 修复回扣**：checkin/force_unlock ABAC 越级 404 钉在（173 含）；RANGES+checkout_hours PUT 白名单（1..8760）绿；sqlx migrate! touch 教训已应用（fresh 库 20 条迁移全跑，0020 生效）。
- **编码**：b1d2da4 提交信息（-F 修正后）与三文件中文全部正常，无乱码；本评审真库探针中出现的乱码系探针侧 Git Bash 管道编码，与服务无关。
- **Dockerfile**：本分支未触碰（diff stat 15 文件无 Dockerfile）✓；迁移 0020 走 sqlx migrate! 运行时链，无镜像改动需求。
- **台账**：计划文档 D1-D3 裁定与实现一致；migrations.md 0020 注册+四裁定（泛化表/ABAC 双端/无环/物料侧 UI 后置）与代码逐条对得上。

## 6. 总判定

**FIX-THEN-MERGE**。后端 T1 质量过硬（四端点/迁移/ABAC/audit/快照/升版语义全部活体实证通过），但 T2 前端关联搜索确定性损坏（C1-1）且「17×2 全绿」声明失实（C1-2）、README curl 组声明未落（I1-2）、存量 SYS-04 钉矛盾（I1-1）——修复轮：C1-1 一行 helper 替换 + I1-1 一行断言修正 + I1-2 补 curl（或勘误）+ 重跑 Playwright 17×2 全绿 + 台账勘误（含 M1-M4 择要），回读验证后可合并。

# 增量20 终审报告（RPT-01 全局搜索 + RPT-03 tantivy 接线）

- **Reviewer**：独立 fresh-eyes（未参与开发）
- **基线**：main @ d70706e（增量19 合并后）；**分支**：feat/global-search
- **对账对象**：aaee453（T1 core-search）/ 98b11c0（T2 网关接线）/ d5df27e（T3 前端）/ ec4916b（T4 文档）
- **终审时间**：2026-11-15
- **总判定**：**FIX-THEN-MERGE**（无 C1；1 条 I1 需修复轮清偿：密级提密后索引陈旧可见窗口 + service.rs 失实注释。其余全数实证通过）

---

## 一、验证手段（全部独立复跑，不信任声明）

| 项 | 结果 | 证据 |
|---|---|---|
| cargo test --workspace（TEST_DATABASE_URL 真库，独立 CARGO_TARGET_DIR=target-review） | **164 通过 0 失败** | 逐套件计数：6+16+4+1+3+6+3+6+12+6+2+6+2+9+11+8+5+15+7+5+31=164（api_gateway 6 / bom_it 16 / dispatch_it 4 / im_it 1+3 / core_files 3 / core_im 6 / core_model 6+12 / core_search 6 单测+2 集成 / core_sec 6+2+9 / core_version 11+8+5+15 / mod_doc 7 / mod_part 5 / e2e 31）。与声明"后端 164"精确一致 |
| cargo clippy --workspace --all-targets | **0 警告** | exit 0，无任何 warning 行 |
| cargo fmt --all -- --check | 通过 | FMT_OK |
| 前端 Vitest | **29 通过** | npx vitest run（unit.spec.ts 29 tests，groupLabel 2 条在内） |
| 前端 vue-tsc --noEmit | 通过（无输出） | 死代码/未用导入静态核查 + 类型核查 |
| Playwright e2e（自起 18095 后端 + vite 5173，独立库 plm_rvw20_e2e） | **12/12 一次全绿（无重试）** | 含 MainLayout 接线后全部既有钉（看板计数联动/角色显隐/待办中心/audit/状态筛选回写）+ 新钉顶栏搜索双组聚类/编号精确行跳详情 |
| 真库探针（plm_rvw20 @ 18096，target-review 产物） | 全链路实测 | 见 §三 |
| OpenAPI 活体对账 | 通过 | /api/docs/openapi.json 实测含 `/api/v1/search` 路径 + SearchResponse/GroupDto/SearchHitDto 三 schema（16 paths/20 schemas） |

探针环境已清理：taskkill plm-server×2 + vite、DROP DATABASE plm_rvw20/plm_rvw20_e2e、临时目录/target-review 已删，git status clean。

---

## 二、逐条声明对账（A 项）

### T1（aaee453）core-search crate — 全部可对账
- tantivy 0.27（Cargo.lock: tantivy-0.27.0）+ jieba-rs 0.11（Cargo.lock: jieba-rs 0.11.0）✅
- 自写 JiebaTokenizer：`crates/core-search/src/tokenizer.rs:46`（jieba `TokenizeMode::Search` 带偏移）+ `:50-58`（CJK 相邻双字 bigram 兜底）；小写化 `:39`；6 条单测 + 2 条真库集成（crate 8 测实测过）✅
- SearchService：rebuild_full 幂等（delete_term+add_document 按 obj_no，service.rs:197-232）/ scan_once 严格水位（`updated_at > $1`，service.rs:262）/ search_groups 聚类（:410-445）/ search_page 钻取分页（:448-479）✅
- ABAC 密级：term 枚举 0..=max 查询过滤（service.rs:317-328）+ hit_of stored 复核（:357-364）✅ 机制存在——但 stored 复核的"防索引陈旧"注释失实，见 I1-1
- 多实例目录锁 degraded：writer 获取失败 → writer=None（service.rs:152-158），写路径 503（upsert_rows → DomainError::ServiceUnavailable → core-model/src/error.rs:142 http_status=503），查询仍可；集成测试 `multi_instance_second_open_is_degraded_readonly` 实测过 ✅
- 弃 QueryParser：core-search 源码零 QueryParser 引用（仅注释），content_query 逐 term 析取（service.rs:287-314）✅
- 纯 ASCII 仅 obj_no 精确：`q.chars().any(is_cjk)` 门控（service.rs:296-297）✅ 活体实证：`q=DWG-RVW-2` total=1 仅图纸
- commit 退避重试（0/300/900ms）：service.rs:213-226；tokio Mutex Guard 跨 await（:203 持锁贯穿 sleep/commit/reload——tokio Guard Send 安全，编译实证）；重试成功后 reader.reload 仍执行（:228-230，last_err=None → break → reload）✅
- Dockerfile 桩层三处齐全：`Dockerfile:16`（COPY core-search/Cargo.toml）、`:23`（mkdir crates/core-search/src）、`:26`（for d in … core-search … stub 循环）✅

### T2（98b11c0）网关接线 — 全部可对账
- GET /api/v1/search：`crates/api-gateway/src/routes/search.rs:85-141`；docs:read 门控（routes/mod.rs:41-43 与 /docs 同层 require_perm("docs","read")）；空 q 400（search.rs:91-97）；非法 type 400（:109-117）；无 type 聚类每型 10 条+total（:100-106 per_group=10）；有 type 平铺分页（:107-131）✅
- AppState.search 字段（state.rs:30-31）；OpenAPI 注册 path + 三 schema（state.rs:82、92-94）✅ 活体对账一致
- main.rs 装配：data_dir/search 索引 + spawn_scanner（main.rs:184-191），PLM_SEARCH_SCAN_SECS 缺省 2s（Config :55 defaults + `Env::prefixed("PLM_")` 小写映射 search_scan_secs，`:63.max(1)` 下限钳制）✅
- harness/degraded_app 同构构造（tests/common/mod.rs 两处 AppState 均补 search 字段）✅
- e2e 钉 9 类齐全（e2e.rs:2660-2797：聚类双组/子串/编号精确/越密级/钻取分页/非法type/空q/401/近实时扫描机制钉）✅

### T3（d5df27e）前端 — 全部可对账
- 顶栏搜索框 MainLayout.vue:39-46（data-test="global-search"，Enter→/search?q=）✅
- /search 聚类页 Search.vue（分组 section + 图纸行"查看"跳详情 :109-113；part 无详情页不出按钮——正确）✅
- api/search.ts（searchGlobal + groupLabel）+ groupLabel 纯函数单测 2 条（tests/unit.spec.ts:233-242）✅
- **计划外清偿（MainLayout 死组件）**：实证 main 上 `git grep MainLayout frontend/src` 零引用——增量19 提交（03f4e04）"路由/菜单全员可见"当时确为死代码；本轮 router/index.ts:8-27 改根布局 children，子路径与既有 URL 逐条一致（/、/docs、/docs/:objNo、/todos、/parts、/admin/users、/admin/audit），guardTarget meta 合并语义（vue-router 4 matched 合并）不受影响；**清偿在计划对账台账有据可查**（plans/2026-11-15-rpt-global-search.md:62"计划外清偿——MainLayout 为死组件（增量19「菜单」声明未生效）"）——对账诚实性达标 ✅
- Workbench/DocsList 等页面渲染：所有页面根为 `div.page`（无嵌套 el-container 冲突），MainLayout.vue:52-54 el-main 内 router-view；Playwright 12 测（含看板/待办/audit/用户管理）独立复跑全绿实证 ✅
- el-select/el-input 键盘导航钉仍稳：flow.spec.ts:34-35、376-377 两处 ArrowDown+Enter 未改动，本轮独立复跑通过 ✅
- Parts 名称筛选：frontend/src/api/part.ts name 参数 + Pages.vue:24/36/124-132（后端 ListPartsQuery.name 增量17 已有，parts.rs:49-51）✅
- Playwright 12 测：本轮独立复跑 **12/12 一次全绿**（声明"两轮全过"，本轮一轮实证；spec 静态核查无破坏性改动）✅

### T4（ec4916b）文档 — 全部可对账
- README：搜索 curl 组/状态行"20 个增量"/前端段 ✅
- 部署文档：PLM_SEARCH_SCAN_SECS 表行/search 端点行/架构图全文索引目录 `/data/search` ✅
- 评审报告：RPT-01 ✅（:114）/ RPT-03 🟡（:116，正文抽取 V1.0 ⬜ 诚实标注）/ 总评 ②→增量20 已接线（:14）✅
- migrations.md：增量20 节七条裁定逐条数齐（分词器自写/弃QueryParser/纯ASCII/对账扫描器/目录锁/无墓碑/AV锁）✅
- 计划对账台账：T1-T4 四条回填含 API 差异/诊断实证/计划外清偿 ✅

---

## 三、真库探针实测（C 项）

环境：plm_rvw20 @ 127.0.0.1:18096（target-review 产物，独立 DATA_DIR），admin 引导 + 建两 editor（上限均 0）。

| 探针 | 结果 |
|---|---|
| 登录/建用户/授权 | ✅ |
| editor 上传图纸「六角轴承座」(DWG-RVW-2) + 建物料 CLS-ZZ（WL-ZZ-00001） | ✅ 201 |
| 近实时：上传后轮询 `q=轴承` 双组命中 | ✅ 双组命中（drawing=1/part=1）；扫描器 2s 周期实测收编 |
| 子串「承座」（jieba 未收录 → bigram 兜底） | ✅ total=1 命中 DWG-RVW-2 |
| 纯 ASCII 编号精确 `q=DWG-RVW-2` | ✅ total=1（分词碎片零泛匹配） |
| 钻取 `type=drawing&page=1&size=1` | ✅ items=1 / total 正确 |
| 空 q / 无 token / 非法 type | ✅ 400 / 401 / 400 |
| OpenAPI 活体 | ✅ 端点+三 schema |
| **SQL 提密一图纸到 2 → 低上限用户 q** | **⚠️ 提密后立即查询仍可见**（陈旧窗口）；**约 10 分钟后周期重建自愈**（低上限 drawing 组归 0，实测 21:01:33 确认）——根因见 I1-1 |

---

## 四、发现清单

### C1（必须修）
无。API 可达面上未发现安全洞或声明失实：应用内**无任何改密级端点**（grep 实证，secret_level 仅创建时落库），业务更新路径（save attrs store.rs:296-299、生命周期 core-version/store.rs:130-138）均 `updated_at=now()`，扫描器可收编；索引陈旧泄露仅经带外 SQL 触发。

### I1（应该修——修复轮清偿后合并）
- **I1-1 密级提密后 ≤10 分钟索引陈旧可见窗口 + service.rs:362 注释失实。**
  - **证据**：真库实测——`UPDATE cm_object SET secret_level=2`（无 updated_at 触发器，core-model 迁移 0001 仅 INSERT DEFAULT now()）后，低上限（0）用户 `q=轴承` 聚类与 `type=drawing` 钻取**两条路径均仍返回该图纸**，直到 ~10 分钟后 spawn_scanner 第 300 轮 rebuild_full 自愈（crates/core-search/src/lib.rs:36-43）。`cm_object` 无 UPDATE 触发器（psql 实证 updated_at 未随 UPDATE 变化）；scan_once 严格水位（service.rs:262）因此漏收。
  - **声明失实点**：service.rs:362 注释"stored 复核（**防索引陈旧**越密级——fail-closed）"——stored 复核读的是**同一份陈旧 tantivy 文档**的 stored 字段（:357-361），对索引陈旧**无任何防护**（查询过滤与复核同源同值），只防查询构造缺陷。评审报告 :114"查询侧+stored 双保险"作为机制描述成立，但"双保险"语义在其唯一声称的威胁场景（陈旧）下不成立。
  - **与验收探针冲突**：本轮验收探针 C 明确要求"SQL 提密一图纸到 2，低上限用户 q 不可见"——实测在窗口内不可见不成立。
  - **最小修复建议**（三选一，前者最轻）：
    1. 文档+注释+测试清偿：改正 service.rs:362 注释（stored 复核仅防查询缺陷，不防陈旧）；migrations.md 增量20 节补一条裁定"带外 SQL 更新（不改 updated_at）在 ≤300 轮（≈10min）内检索视图可见旧密级——其余视图（DB 直读）即时收敛；应用内无改密级端点，窗口仅带外可触发"；e2e 补钉：先 scan_once 索引 → SQL 提密 → 再 scan_once → 低上限不可见（现 e2e 是提密后才首扫，未覆盖已索引后提密的回放路径，total 计数一致性也一并钉住）。
    2. 硬化（推荐，改动小）：hit_of 命中行按 obj_no 回查 DB secret_level 复核（每页 ≤100 行，两次轻查询；查询侧 term 枚举保留为第一道闸）。此法同时消解 total 计数含陈旧密级文档的偏差。
    3. 备选：cm_object 加 UPDATE 触发器 bump updated_at（根治带外更新不可见，但违反本轮"无新迁移"裁定，须台账记并重评）。

### M1（建议）
- **M1-1 secret_filter 对 max_secret<0 退化为 match-all**（crates/core-search/src/service.rs:317-328）：`(0..=i64::from(max_secret))` 空 range → 空 BooleanQuery，tantivy 0.27 空布尔查询=match-all 语义（tantivy-0.27.0/src/query/boolean_query/boolean_weight.rs:126-133 "restore the match all semantics if the list becomes empty"）。现网不可达（建用户校验 0..=3，core-sec/service.rs:226-229）且 stored 复核兜底不泄露，但建议一行钳制 `let max_secret = max_secret.max(0)` 防未来数据路径变化引入 fail-open。最小修复：service.rs:318 range 起点前钳制。
- **M1-2 计划台账含西里尔字母**：docs/superpowers/plans/2026-11-15-rpt-global-search.md:60 "jеiba"（е 为 U+0435）——检索/复制/对账踩坑，改 ASCII "jieba"。
- **M1-3 Playwright 测试内死变量**：frontend/tests/e2e/flow.spec.ts:492 `const PT` 传入 page.evaluate（:531 `{ NO, PT }`）但闭包体未用——删除或用于物料 obj_no 断言（物料编号服务端生成，现仅断言 status 201，用 PT 断言会更强）。
- **M1-4 菜单高亮**：MainLayout.vue:27 `:default-active="$route.path"` 在 /docs/:objNo 详情页不高亮"图纸"项（router 精确匹配）——新布局才暴露的既有语义，可用 computed 前缀映射，纯 UX 可后置。

---

## 五、历史易犯错误专项核查（D 项，逐项）

| 项 | 结论 |
|---|---|
| OpenAPI 完整性 | ✅ 代码注册（state.rs:82/92-94）与活体 openapi.json 一致（端点+三 schema 实测在） |
| ABAC 两路径 | ✅ 聚类与钻取共用 combined()（查询过滤）+ hit_of（stored 复核）——同源同值；e2e 与真库探针（索引新鲜态）均过。窗口期缺陷见 I1-1 |
| Dockerfile 桩层 | ✅ COPY(:16)/mkdir(:23)/stub 循环(:26) 三处齐全 |
| 迁移注册表 | ✅ `git diff main..feat/global-search --name-only | grep migrations` 仅 docs/migrations.md |
| 增量19"菜单全员可见"清偿对账 | ✅ main 上 MainLayout 零引用（死代码实证）；清偿入台账（plans/:62）；router children URL 全保留；Playwright 既有 11 钉+新 1 钉全绿实证无破坏 |
| scan_once 严格水位+迟到行兜底 | ✅ lib.rs:37 `scans.is_multiple_of(300)` → rebuild_full 真实存在；真库实测自愈生效 |
| commit 退避重试 | ✅ tokio Mutex Guard 跨 await（Send 安全，编译+164 测试实证）；重试后 reader.reload 仍执行（:228-230） |
| 前端 dead code | ✅ Search.vue/MainLayout.vue/Parts.vue/api/search.ts 无未用导入（逐一核对 + vue-tsc 干净）；groupLabel 纯函数 2 单测 |
| el-select/el-input 键盘导航钉 | ✅ 两处 ArrowDown+Enter 钉未动，本轮 Playwright 独立复跑通过 |

---

## 六、总判定

**FIX-THEN-MERGE**

理由：T1-T4 全部声明与代码/文档可对账，五套测试（后端 164 / clippy 0 / fmt / Vitest 29 / Playwright 12）独立复跑全绿，真库探针除"提密即时不可见"外全过。唯一拦点是 I1-1——密级提密后 ≤10 分钟检索视图陈旧可见（且 service.rs:362"防索引陈旧"注释失实，触碰对账红线）。该问题带外 SQL 才可触发、周期重建自愈已实证，修复轮按 I1-1 最小建议（文档/注释/e2e 清偿，或 hit 时 DB 复核硬化）清偿后即可合并；M1-1/M1-2/M1-3 顺手同轮带上（均为一行级）。

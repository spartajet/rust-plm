# 增量20：RPT-01 全局搜索框 + RPT-03 tantivy 接线

> 需求评审 P0-A 第 3 项。PRD §6.12：RPT-01 全局搜索（一处搜一切、按类型聚类、权限过滤）+ RPT-03 全文引擎（内置 tantivy、索引近实时 <5s）。本增量 MVP 口径=评审报告裁定：「属性+文件名起步，正文抽取（PDF/DWG 标题栏）V1.0」。
> 分支：`feat/global-search`；无新迁移；**新 crate core-search（Dockerfile 桩层必须同步——三次事故纪律）**。

## 设计方案与裁定（ADR-0005 对齐）

### D1 引擎与分词
- **选定**：内嵌 tantivy 0.27（ADR-0005 已定案：零额外部署、单机千万级 P95<200ms）；**自写 JiebaTokenizer**（jieba-rs 0.11 适配 tantivy Tokenizer trait，~40 行）——社区 tantivy-jieba crate 停更于旧版（0.27 无适配），自写保留 ADR 的 jieba 分词语义，依赖面最小。
- 否决：PG trigram/tsvector（违背 ADR-0005 裁定）；Elasticsearch/Meilisearch（部署重）。
- 拼音/同义词/驼峰（RPT-01 全量）：后置，台账记（分词器 trait 化，后续插 synonym filter）。

### D2 索引与近实时
- 索引目录 `PLM_DATA_DIR/search/`；schema：type_code(STRING)+obj_no(STRING 精确)+text(TEXT jieba=name+material 等短文本属性)+state/major/minor/secret_level(STORED 展示+过滤)。
- **更新管道=对账扫描器（ADR「索引任务+对账修复」的 MVP 形态）**：启动全量重建（幂等 delete+add）；随后每 2s（`PLM_SEARCH_SCAN_SECS` 可配，测试注入 0.2s）扫 `updated_at > 水位-1s 重叠窗` 的最新活版本（DISTINCT ON），按 obj_no delete_term+add 幂等 upsert，commit 后推进水位。<5s 达标。
- 否决：outbox 事件驱动（需为 create 加 doc.created/part.created 主题+每写入路径改动，MVP 收益低——扫描器零侵入写入链路；事件驱动留作演进记台账）。
- 删除一致性：应用无删除端点（软删 deleted_at 恒 NULL），扫描器不处理墓碑（台账记）。
- **多实例裁定**：tantivy 目录锁单写者——第二实例打开失败→SearchService 进入 degraded，/search 返回 503 fail-closed（不崩服务）；单机承诺内（ADR-0005 后果段）。

### D3 权限
- ABAC 密级：查询侧 `secret_level ≤ 用户上限` RangeQuery+stored 校验双保险（不泄露存在性——越密级不出结果集）。ADR 的 bitset 预计算针对对象级 ACL，当前系统无该维度，后置台账记。
- 门控：docs:read（与 GET /docs、GET /parts 同层）。

### D4 API 与聚类
- `GET /api/v1/search?q=...&type=drawing|part&page=&size=`：q 必填（trim 空串 400）；无 type→按类型聚类 groups（各限 10 条+total）；有 type→平铺分页钻取。响应 `{q, groups:[{type_code,total,items:[{obj_no,name,state,major_ver,minor_ver,secret_level}]}], total}`。
- 匹配域：obj_no 精确 term ∪ text 字段 jieba 分词 BM25（name+material 起步）。

### D5 前端
- MainLayout 顶栏搜索框（Enter → /search?q=）+ `Search.vue` 分类型 section 渲染（图纸行点击跳详情；物料行展示，无详情页）；路由 /search。

## 任务清单（TDD）

### Task1 core-search crate（引擎内核）
- RED：`core-search/tests/search_it.rs`（临时真库样板仿 mod-part）——建 drawing/part 行（直插 cm_object）→ rebuild_full → search("轴承") 双类型命中；"承座" 子串命中（jieba 分词）；密级过滤；obj_no 精确命中；分词器单测（jieba 切分断言）。
- GREEN：lib.rs（SearchService::new(dir)/rebuild_full(pool)/scan_once(pool,&mut watermark)/search(q,types,max_secret,limit)）+ tokenizer.rs（JiebaTokenizer）+ 可选 degraded 状态。
- 注意：tantivy 0.27 API（IndexWriter 单写者、RangeQuery、TopDocs）；clippy；workspace members + Dockerfile 桩层同步。

### Task2 网关接线 + e2e
- RED：plm-server/tests/e2e.rs `search_global_clusters_and_abac`——上传图纸（名轴承座）+建物料（名六角轴承）→ 轮询 ≤3s（近实时钉）→ GET /search?q=轴承 双聚类；子串/编号精确；越密级用户不见 level-2 件；空 q 400；无 token 401；type 钻取分页。
- GREEN：state.rs AppState.search 字段+routes/search.rs（docs:read 组）+OpenAPI 注册+main.rs spawn 扫描器（interval 读配置）；harness migrated/degraded 两套构造同步。

### Task3 前端
- MainLayout 顶栏搜索框+Search.vue（分类型 section+行点击）+路由；Playwright 钉（顶栏输入→结果双 section→图纸行跳详情）；Vitest 如有纯函数。

### Task4 文档与对账
- README curl 组；部署文档 §9+环境变量 `PLM_SEARCH_SCAN_SECS`；评审报告 RPT-01/03 行更新；migrations.md（无新迁移+裁定：扫描器形态/多实例 degraded/拼音同义词后置/无删除墓碑）；计划对账台账回填。

### Task5 终审与合并
- fresh reviewer（真库探针+对账）→ 修复轮 → squash merge main → SSH 推送 → **先跑 cargo fmt --check 本地预检（增量19 CI 缺口教训）** → CI 绿。

## 验收标准
- 后端 e2e 新增 ≥6 钉全绿；`cargo test --workspace` 全绿（基线 155）；clippy 0 新警告；Vitest/Playwright 全绿（基线 27/11）；OpenAPI 含 search 端点与 schema。
- 近实测钉：上传后 ≤3s 搜索可命中（测试内轮询断言）。
- Dockerfile 含 core-search 桩层；CI（fmt/clippy/test/docker）全绿。

---

## 对账台账（执行后回填）

- T1 完成（aaee453）：tantivy 0.27 API 与 ADR 时代（0.22）差异大——BooleanQuery 元组序翻转/add_string_field 移除/Index::exists 需 Directory/commit &mut self（writer 改 tokio Mutex）；QueryParser PhraseQuery 语义诊断实证后弃用（改逐 term 析取）；jieba tokenize 用 Search 模式带偏移。
- T2 完成（98b11c0）：e2e BM25 排序不稳定→集合断言；harness 暴露 search 句柄供近实时钉。
- T3 完成（d5df27e）：**计划外清偿——MainLayout 为死组件**（增量19「菜单」声明未生效）→ router 改根布局 children 接线，既有 URL 保留；board 测试初渲竞态（全串匹配钉）；纯 ASCII 混合查询语义（分词碎片泛匹配诊断实证）；Windows AV 锁→commit 退避重试+e2e 数据目录移出项目树；parts 测试数据累积挤出第一页→Parts 页加名称筛选（UX 增强）。
- T4 完成：README/部署文档/评审报告（RPT-01 ✅、RPT-03 🟡）/migrations.md 七条裁定已更新。

### 终审修复轮（FIX-THEN-MERGE → PASS）
- I1-1 清偿：**DB 密级权威复核**——search_groups/search_page 命中行回查最新活版本 secret_level（每页 ≤100 行），带外提密/删除立即剔除 fail-closed（不等索引自愈）；total 仍索引侧计数（偏高方向安全，注释明示）；失实注释更正；e2e 新钉「带外提密→立即不可见（low）/admin 不受影响」。
- M1-1：secret_filter max<0 → 恒假 term（空 BooleanQuery=match-all 陷阱）。
- M1-2：计划文档西里尔 е → ASCII。
- M1-3：flow.spec PT 死变量删除。
- M1-4（后置记账）：default-active 在 /docs/:objNo 不高亮图纸菜单——纯 UX。
- 修复轮后：fmt/clippy/后端164/前端29/Playwright 12 两轮全绿（parts 查重确认弹窗条件处理——历史 run material=304 累积触发 duplicates OR 匹配的又一数据累积钉）。

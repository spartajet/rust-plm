# 增量21：BOM 编辑 UI（BOM-01 切片）+ BOM-07 汇总报表

> 需求评审 P0-A 第 4 项。PRD §6.5：BOM-01 EBOM 管理（结构树增删改+行级属性）P0、BOM-07 汇总报表（用量汇总/自制外购分类汇总/成本估算/导出）P0。评审现状：BOM 引擎满血（set_lines/expand 三模式/as-of）但**前端无编辑界面、汇总全缺**。
> 分支：`feat/bom-editor-summary`；无新迁移；无新 crate。

## 范围裁定（MVP 切片与后置）

- **本增量做**：①BOM 行富读端点+前端结构编辑器（改 qty/增行/删行/保存=全量 set_lines，循环 409/工作态语义与引擎现状对齐）②BOM-07 用量汇总（leaf 多路径 qty_total——数据源即 expand LeafOnly 语义）+自制/外购分类汇总（part attrs.class_code/class_name）+CSV 导出 ③as-of（?major= 快照域，与 expand 同构）④core-search SearchHit 增 obj_id（编辑器选件需 uuid；索引目录版本化 search-v2）。
- **后置（台账记）**：行级属性扩展（单位/基准/备注/损失率——bom_edge 无列需迁移 0017，与 Excel 双向/复制粘贴/拖拽同批）；成本估算（物料无价格属性）；MBOM 多视图（BOM-04 V1.5）。

## 设计要点

### D1 富行端点（编辑 UI 数据源）
`GET /api/v1/boms/{obj_no}/lines?major=N`（docs:read）→ `{lines:[{line_no, child_id, child_obj_no, child_name, qty}]}`：
- live 域=bom_edge（按 line_no 序）；as-of 域=bom_edge_snapshot（release_root+major，快照无 line_no 稳定序→按 child 序+行号回填 0 占位——快照行仅读）。
- 子件 join cm_object（obj_no/attrs->>name）+ **ABAC 子件密级过滤**（secret_level ≤ 上限；越级行不泄露）。
- core-bom 新方法 `lines_info(parent, snapshot: Option<(Uuid,i32)>)`。

### D2 汇总端点（BOM-07）
`GET /api/v1/boms/{obj_no}/summary?major=N[&format=csv]`（docs:read）：
- 用量汇总=LeafOnly 语义（bom_path per-descendant SUM(qty_path)；as-of=快照域 qty SUM）；join 子件 cm_object（obj_no/name/secret ABAC）+ LEFT JOIN part 域 attrs 的 class_code/class_name（图纸子件分类为空）。
- JSON `{rows:[{obj_no,name,class_code,class_name,qty_total}]}`；`format=csv` → text/csv（Content-Disposition attachment；UTF-8 BOM 兼容 Excel）。
- core-bom `summary(parent, snapshot)` 返回 Vec<SummaryRow>。

### D3 前端编辑器（DocDetail BOM 卡片）
- 「编辑结构」按钮（canWrite）→ 编辑态表格：qty 可编辑 input、行删除、**添加子件=全局搜索对话框**（搜名称/编号→选行→取 SearchHit.obj_id，去重校验）→「保存」=全量 POST lines（line_no 重排 1..n）→ 成功重载；409 循环→错误提示。as-of 快照视图只读（编辑按钮隐藏）。
- 「汇总」折叠面板：用量汇总表（编号/名称/分类/单台用量）+分类小计行+「导出 CSV」按钮（window.open 带 Bearer? CSV 下载用 fetch+blob）。

### D4 core-search obj_id
- schema 加 obj_id(uuid STORED)；ScanRow 取 o.id；SearchHit.obj_id。
- **索引目录版本化**：main.rs 装配目录改 `data_dir/search-v2`（schema 变更不兼容旧索引——一次性废弃重建，v1 目录残留无害，台账记）；harness 同步。

## 任务清单（TDD）
1. **T1** core-search obj_id+目录 v2：单测/集成测试更新（SearchHit.obj_id 断言）→ GREEN → Dockerfile 不动（无新 crate）。
2. **T2** core-bom lines_info/summary + 路由三端点 + OpenAPI + e2e 钉（富行/ABAC 子件越级隐藏/汇总多路径 qty/分类/as-of 快照/CSV 头）。
3. **T3** 前端编辑器+汇总+Playwright 钉（编辑保存→onelevel 复验、汇总分类行、CSV 下载 blob 前缀钉）。
4. **T4** 文档：README/部署文档端点表/评审报告（BOM-01 🟡→✅ 切片、BOM-07 ⬜→🟡）/migrations.md 裁定/台账。
5. **T5** fresh reviewer 终审 → 修复轮 → fmt 预检 → 合并 → SSH 推送 → CI 绿。

## 验收标准
- e2e 新增 ≥6 钉全绿；后端 164+ 全绿/clippy 0/fmt 过；前端 Vitest 29+/Playwright 12+ 两轮全绿；OpenAPI 注册三端点与 schema。
- CSV 下载头含 Content-Disposition 与 BOM；汇总 qty 与 expand leaf 一致（同数据双端点交叉断言）。


---

## 对账台账（执行后回填）

- T1 完成（c23c6aa）：obj_id uuid STORED+目录 v2+uuid 依赖。
- T2 完成（a55eb14/adcdc8b）：快照 SQL 占位符冲突（$2 既 major 又 secret——e2e 拦下）；快照存在性校验补齐（无快照 major 400）；BOM 路由父件限 drawing（part 父件后置台账）；多路径汇总语义钉（直挂3+经子装配5=8）。
- T3 完成（e053ad7）：**两处回归被既有钉拦下**——watch(bomMajor) 重写时丢 reloadBom（bom version switch 钉）、网关 SearchHitDto 漏 obj_id（bom editor 钉）；parts 查重确认弹窗条件处理系增量20 修复轮（9910594）已落地——本增量仅沿用（原台账误记 T3 名下，终审 I1-2 修正）。
- T4 完成：README/部署文档/评审报告（BOM-01 🟡 增补编辑器、BOM-07 ⬜→🟡）/migrations.md 五条裁定。
- 修复轮：见终审报告 docs/review-增量21-终审.md。

### 终审修复轮（PASS → 清偿 I1×2+M1×3）
- I1-1：「发布快照只读」提示移出 bomEditable 容器（原死分支——as-of 用户得不到只读提示）+ data-test 钩子。
- I1-2：台账归属修正（parts 弹窗处理系增量20 9910594）。
- M1-1：e2e 陈旧注释（13→8 语义）更正。
- M1-2：CSV 公式注入防护（=+-@/\t/\r 开头加 ' 前缀）。
- M1-3：major 切换时展开态汇总面板自动刷新（summaryOpen v-model + watch 分支）。
- M1-4/M1-5 记账后置：obj_id STORED 分词噪声（下次 schema 变更顺带 STRING|STORED）；set_lines/expand/where-used 三老端点 OpenAPI 未注册（先于本增量，后续增量补）。
- 修复轮后：后端 165/clippy 0/前端 32/Playwright 13 全绿。

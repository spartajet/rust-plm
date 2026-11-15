# 增量19：WF-03 待办中心 + 审批入口

> 需求评审 P0-A 第 2 项。PRD §6.7 WF-03：「任务处理：统一待办中心（待办/已办/我发起）；审批意见必填开关；附件查看；批量审批（同流程多对象）」，P0。
> 分支：`feat/todo-center`；无新迁移（复用 audit_log 与 cm_type.config）；无新 crate（Dockerfile 桩层不动）。

## 设计方案与裁定

### D1 待办数据源
- **方案 A（选定）**：零新表查询——待办=`cm_object` 中 `state IN ('review','checked') AND deleted_at IS NULL AND secret_level≤上限 AND (updated_by IS DISTINCT FROM me)`；我发起=`created_by=me`（每 obj_no 取最新活版本 `DISTINCT ON`）；已办=`audit_log`（增量18 表）中 `username=me AND action IN (submit,approve,reject,release,obsolete,revise) AND object NOT NULL`。
- 方案 B（否）：新建 wf_task 实例表（提交生成任务行）。为 WF-05 超时催办/ORG-06 代理移交打底，但属 EC/V1.0 域；本增量 PRD 只要求入口+意见+批量。裁定：A，B 入台账为后续演进。
- 语义钉：strict 档 maker-checker（checked→approve 排斥 updated_by/created_by）与「待办=updated_by≠me」天然呼应——待办列表永不列出自己无权批的二级任务。
- 附件查看=行点击跳 `/docs/{obj_no}` 详情页（已有文件信息+下载）。

### D2 审批意见
- 六个 lc 端点 body 扩展为 `Option<Json<{comment: Option<String>}>>`（axum Option extractor：无 body/无 content-type → None，向后兼容，存量测试与 IM 回调不受影响）。
- 意见落 `audit_log`：action=事件名、object=obj_no、`detail={comment,from,to}`——同时满足 SYS-01 审批留痕与 WF-03 已办数据源。落点：网关 `do_transition`（旧状态在 ABAC detail 预读时可得）+ `im.rs` 回调（无 comment）。audit_mw 的 http 兜底行不冲突（action 名不同，已办查询按事件名过滤）。
- **必填开关**：`cm_type.config.require_comment`（同 flow_tier 的 `config || jsonb_build_object(...)` 模式）；扩展现有 `PUT /meta/types/{code}/flow-tier` body 为 `{tier?, require_comment?}`（至少一项，tier 仍校验合法值）。强制点=网关 `do_transition` + batch-approve（approve/reject 缺意见 → 400 `CommentRequired`）；IM 回调豁免（卡片无意见输入，HMAC 可信路径——台账记裁定）。

### D3 批量审批
`POST /api/v1/docs/batch-approve`，body `{obj_nos: [..], comment?}`；docs:approve 门控（docs_lc_approve 组）；顺序逐个 transition（同 do_transition 语义：ABAC 404/意见必填/maker-checker），逐项收集 `{obj_no, ok, error?}` 不中断批次；上限 50（超出 400）。PRD「同流程多对象」= 同类型待办态，不符者逐项报错。

### D4 前端
`Todo.vue`：三 tab（待办/已办/我发起）+ 密级/状态列（与 DocsList 同配色）；待办 tab checkbox 多选 + 批量审批按钮（canApprove 显隐）+ 意见输入（开关开启时必填提示）；行点击跳详情。路由 `/todos` + 菜单「待办中心」全员可见（数据面由 docs:read + ABAC 兜底）。逐项结果用 ElMessage/表格展示。

## 任务清单（TDD）

### Task1 后端待办三盒查询（mod-doc DocService::todos + 路由）
- RED：`plm-server/tests/e2e.rs` 新增——editor 提交后 admin `GET /api/v1/todos?box=todo` 见该件（含提交人 username/attrs.name/state）；`box=mine`（editor 视角）见自己发起；approve 后 `box=done` 见 approve 审计行；越密级不出现在他人待办。
- GREEN：`DocService::todos(box, me: Uuid, max_sec, page, size)` 三段 SQL（DISTINCT ON 去重、JOIN sec_user 取提交人名、已办 JOIN cm_object 补当前状态与 attrs.name）；`routes/todos.rs` GET 端点挂 docs_r 组（docs:read）；OpenAPI 注册。
- 复核：ABAC 钉、分页 total、`updated_by IS DISTINCT FROM me` 处理 NULL。

### Task2 审批意见 + 必填开关
- RED：①POST approve 带 `{"comment":"ok"}` → 200 且 audit_log 出现 action='approve' detail 含 comment/from/to；reject 同。②PUT flow-tier `{require_comment:true}` 后 approve 无意见 400 CommentRequired、带意见 200；tier 与 require_comment 同改生效。③无 body 的 approve 照旧 200（向后兼容钉）。④IM 回调 approve 无意见仍 200（豁免钉）。
- GREEN：lc_endpoint! 宏加 `Option<Json<LcBody>>`；do_transition 强制开关 + audit.record（action=ev.as_str()）；meta.rs set_flow_tier 扩展；DomainError 加 CommentRequired（若有必要，或复用 BadRequest 带稳定 msg——按现有 error.rs 风格定，e2e 按响应 code 断言）。
- 注意：audit 埋点放 transition 成功后；detail.from 取预读 state。

### Task3 批量审批端点
- RED：两件待办批量 approve → 两项 ok:true 且 released；含不存在编号 → 该项 ok:false error=404 语义、其余成功；>50 → 400；无 docs:approve 角色用户 → 403；require_comment 开启且无意见 → 全部 ok:false。
- GREEN：`routes/docs.rs` batch_approve handler（复用 do_transition 内部逻辑抽出的 helper）+ docs_lc_approve 组路由 + OpenAPI 注册 + state.rs docs paths。
- 复核：逐项结果 DTO `BatchResult { obj_no, ok, error }` schema 注册（M1 纪律）。

### Task4 前端待办中心
- Todo.vue（三 tab/多选/批审/意见/跳转）+ router `/todos` + MainLayout 菜单 + `api.ts` todos 接口；状态 tag 复用 DocsList 配色；Vitest：todo 列表映射/canApprove 显隐逻辑（若现有单测有同类样板）。
- Playwright 钉（flow.spec.ts 增）：browser fetch 建 editor 用户+授权→editor 登录提交（或 admin 造数+editor 提交）；admin 开 `/todos` 见待办行（断言提交人/名称）→选行填意见批审→断言跳转详情后 state=released；`box=done` tab 见 approve 记录；require_comment 开启时意见框必填拦截（前端校验）。
- 环境对策沿用：browser fetch 直连 API、retries=2。

### Task5 文档与对账
- README 端点表 + 部署文档 §9 端点清单加 `/todos`、`batch-approve`、flow-tier body 扩展说明；migrations.md 注明「本增量无新迁移」；docs/review-需求符合度评审.md WF-03 行更新为 ✅；台账 docs/superpowers/ledger 补三条裁定（方案 B 后置/IM 豁免/audit 双行共存语义）。

### Task6 终审与合并
- fresh reviewer（general-purpose agent）：真库探针实证 + 对账（声明 vs 代码）。
- 修复轮 → squash merge main → SSH 推送（`git push ssh://git@ssh.github.com:443/spartajet/rust-plm.git main`）→ CI 绿。

## 验收标准
- 后端 e2e 新增 ≥8 钉全绿；`cargo test --workspace` 全绿（基线 152+）；前端 Vitest 全绿（基线 24）；Playwright 新钉过（retries=2）；clippy 无新警告。
- OpenAPI（/api/docs/openapi.json）包含 todos/batch-approve 且 body schema 注册。
- 不新增迁移文件；Dockerfile 不动；IM 豁免有测试钉。

# 增量26：DOC-03 检入/检出（独占检出锁）

> 需求评审 P0-B（第 9 项）。PRD §6.4 DOC-03：独占检出锁+锁标记；检出超时自动回收；离线包后置。评审现状：无任何锁机制——两人可同时改同一图纸（并行编辑冲突无防护）。
> 分支：`feat/doc-checkout-lock`；**迁移 0018（mod-doc）**：doc_lock 表；无新 crate。

## 设计裁定

### D1 锁模型
- `doc_lock(object_id uuid PK REFERENCES cm_object(id), by_user uuid NOT NULL, by_name text NOT NULL, locked_at timestamptz NOT NULL DEFAULT now(), locked_until timestamptz NOT NULL)`——独占一行；到期惰性回收（检查时过期即删行视为无锁；定时清理器后置）。
- 时长参数：sys_param `checkout_hours` 缺省 24（复用参数中心；种子 INSERT 0018）。
- **拦截面（核心价值）**：被检出的对象——①upload_doc 同 obj_no 就地升版 → 409 `MDL-DOC-LOCKED`（检者本人放行）②transition Revise → 同 409。拦点在 mod-doc upload_staged/网关 revise 前查锁（锁检查在 mod-doc service 层——网关/IM 回调两路共用）。
- checkin 仅解锁+留痕（文件更新走既有 revise/upload 链——锁是编辑权协调不是版本操作）。
- 管理员强制回收：DELETE /docs/{no}/lock（sec:manage）——audit `doc.force-unlock`。

### D2 端点
- `POST /api/v1/docs/{no}/checkout`（docs:write；ABAC 越级 404）→ 200 `{by,until}`；已锁 → 409 `MDL-DOC-LOCKED`（detail 带持有者/到期）。
- `POST /api/v1/docs/{no}/checkin`（docs:write；仅持有者，非持有者 409）→ 204。
- `GET /api/v1/docs/{no}/lock`（docs:read）→ `{locked, by, until}` 或 `{locked:false}`（detail 已有对象信息；锁信息独立端点供前端轮询）。
- `DELETE /api/v1/docs/{no}/lock`（sec:manage）→ 强制回收。
- audit：doc.checkout / doc.checkin / doc.force-unlock。

### D3 前端（DocDetail）
- 锁横幅：被检出时显示「🔒 {by_name} 检出中 · 至 {until}」（本人检出=绿色可检入；他人=橙色+上传/升版按钮禁用提示）。
- 按钮：检出（working 态+未锁）/检入（本人持有）/强制解锁（canManageSec）。
- Playwright 钉：admin 检出→editor 视角锁横幅+上传禁用提示→admin 检入→横幅消失。

## 任务（TDD）
1. **T1** 迁移 0018+DocService 锁四方法+拦截面+四端点+OpenAPI+e2e 钉（检出/冲突 409/本人放行 upload/checkin 非持有者 409/force unlock/过期惰性/越级 404/audit 三行/拦截面 revise）。
2. **T2** 前端锁横幅+按钮+Playwright 钉。
3. **T3** 文档：README/部署文档端点表/评审报告 DOC-03 ⬜→🟡/migrations.md 0018 注册+裁定/台账。
4. **T4** 终审 → 修复轮 → fmt 预检 → 合并 → SSH 推送 → CI 绿。

## 验收
- e2e 新增 ≥6 钉；后端 171+ 全绿/clippy 0/fmt 过；Vitest 36+/Playwright 16+ 两轮全绿；OpenAPI 注册四端点（快照基线同步更新——TQ-02 门禁实证首秀：契约变更走显式基线刷新）。

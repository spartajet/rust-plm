# 增量4：core-events Webhook 外投 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。

**Goal:** event_outbox 中 pending 事件经投递循环推送到订阅 Webhook（HMAC 签名 + 指数退避重试 + 死信），订阅管理 API（sec:manage）。

**短设计（Approaches）:** A（选定）进程内投递循环拉 pending 行→按订阅扇出→成功标 dispatched/失败退避重试→超限死信（ADR-0007 概设 §3.7）；B 独立投递进程（违背单体内嵌）；C LISTEN/NOTIFY 直推（丢消息面，Outbox 语义弱化）。

**Spec:** ADR-0007、概设 §3.7、PRD INT-02。

## Global Constraints

- 投递：HMAC-SHA256 签名头 `X-RustPLM-Signature`（secret=订阅密钥）+ `X-RustPLM-Topic`；body=outbox payload
- 重试：指数退避 1m/5m/30m 共 3 次 → 超限 `status=2`（dead）
- 幂等：投递成功幂等标记（status 0→1 CAS）；死信可查（GET /api/v1/sec/webhooks/dead）
- 订阅：`POST /api/v1/sec/webhooks`（url/topics[]/secret，sec:manage）；`GET` 列表；`DELETE` 删
- 权限：sec:manage；auth 16KB 上限同 M2

## Review Focus

1. 签名可验证（接收端同 secret 重算一致）→ 集成钉（in-test 接收端）
2. 失败重试后退避、3 次后死信、成功后不再投 → 集成钉
3. 订阅 topic 过滤：只投订阅的 topic → 集成钉
4. 非管理员管理 webhook → 403 → e2e 钉
5. 事件体=签名体（字节一致）→ 集成钉

## Tasks

### Task 1: core-events crate（表 0008 + 投递循环 + 订阅仓储）
- Step1 失败测试 `tests/dispatch_it.rs`：in-test axum 接收端（记录请求+可切换 200/500）→ 建订阅 → lifecycle release 产生事件 → 投递循环跑 → 断言收到+签名一致+status=1；失败路径：接收端 500×3→退避加速配置→死信 status=2；topic 过滤。
- Step2 RED；Step3 实现（`core_events::migrator()` 0008_webhooks.sql；`EventDispatcher::new(pool,client)`+`spawn(interval_ms)`；`WebhookRepo`）；Step4 GREEN；Step5 Commit

### Task 2: 网关订阅管理路由 + 装配
- Step1 `routes/webhooks.rs`（create/list/delete/dead，sec:manage，16KB）；plm-server 装配 dispatcher.spawn(1000ms)；Step2 测试不回归；Step3 Commit

### Task 3: e2e 钉 + 门禁 + 终审 + 合并
- Step1 e2e：in-test 接收端 + 建订阅 + 发布 → webhook 收到 doc.released（Review Focus 全钉）；非 admin 403；Step2 全门禁；Step3 README；Step4 Commit；Step5 终审→修复→合并

## Self-Review
覆盖 INT-02 最小闭环/ADR-0007 投递语义 ↔ Task1-3；签名/重试/过滤/权限 ↔ 钉子 ✓；类型一致 ✓；比例 ✓

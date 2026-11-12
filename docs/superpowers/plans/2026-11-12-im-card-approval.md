# 增量10：IM 卡片审批（钉钉/企微/飞书）实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 用户"一直往前做+按 superpowers 流程稳步推进"常设指令下自主决策执行；fresh reviewer 终审为质量门。

**Goal:** 对象进入评审（review/checked）时向 IM 群推送审批卡片（钉钉加签/企微/飞书机器人 webhook，真实 HTTP）；IM 侧动作经统一 HMAC 回调端点 `/api/v1/im/actions` 回流生命周期（映射 PLM 用户+casbin 门控+密级 ABAC）——群内可感知、可审批，闭环可测（真实企业应用凭证联调后置）。

**短设计（Approaches）:**
- **A（选定）机器人 webhook 出站 + 统一 HMAC 回调契约**：出站用三平台群机器人 webhook（无需企业应用即可发卡——钉钉自定义机器人加签、企微 markdown、飞书文本）；入站动作统一 HMAC 头契约（真实 IM 卡片按钮经企业应用回调网关转发到此端点——凭证联调后置，契约先行为真）。事件管道复用 event_outbox（新增 doc.submitted/doc.checked 主题），IM 消费者以 `im_card_log(event_id UNIQUE)` 订阅（**不依赖 outbox status**——webhook dispatcher 会认领置位，双消费者必须解耦），退避/死信镜像 dispatcher 语义。
- B（否）三平台企业应用 Stream/长连接 SDK 直连：需要真实凭证与网络出口，且 SDK 重量级——违背凭证未备的现实。
- C（否）复用 webhook subscriber 机制（自注册内部回调）：循环自调用、认证绕圈，且 subscriber 的 HMAC 语义与 IM 动作签名混叠——拒绝。

**Spec:** PRD §7.4 OA/IM 对接、ADR-0007（事务性 Outbox）、增量4 webhook 退避/死信语义、ADR-0006（axum+utoipa）。

## Global Constraints

- **事件**：`transition` 落 `doc.submitted`（→review）与 `doc.checked`（strict 二级，→checked）outbox 行，payload 含 type_code/obj_no/major/minor/actor/timestamp。
- **出站**：`CardSender` trait 三实现（dingtalk=markdown+加签 timestamp&sign、wecom=markdown、feishu=text）；`none`=noop 缺省。卡片含 编号/版本/状态/提交人/[前往评审] 链接（`im_doc_url_template` 含 `{obj_no}` 占位）。
- **消费**：CardPusher 1s 轮询订阅 event_outbox → 发送 → status 1；失败 attempts+1 退避 [60,300,1800]，MAX_ATTEMPTS=4 死信（镜像 webhook dispatcher）。
  - **订阅 SQL 终审 I1 裁定（原计划水位线方案作废）**：bigserial id 于 INSERT 分配、提交时可见，并发事务提交序倒挂下「id > max(event_id) 水位线」会永久跳过后提交的低 id 行（审批卡静默丢失，真库实证）——改用**反连接** `WHERE topic IN (...) AND NOT EXISTS (SELECT 1 FROM im_card_log l WHERE l.event_id = o.id) ON CONFLICT (event_id) DO NOTHING`（幂等仍锚定 event_id UNIQUE）。
- **入站**：`POST /api/v1/im/actions`（免 Bearer，16KB 限）：`X-IM-Sign` = HMAC-SHA256(im_secret, body) hex，不匹配 401；body `{obj_no, action: approve|reject, user, comment?}`；`im_user_map`(JSON) 映射 IM id→PLM username，未映射 403；`SecService::enforce(username,"docs","approve")` 不过 403；密级 ABAC 越级 404（与 docs 路由同语义）；迁移 `LifecycleEvent::Approve/Reject` 返回新状态 JSON。
- **配置**（figment PLM_ 前缀）：`im_provider`(none|dingtalk|wecom|feishu 缺省 none)、`im_webhook`、`im_secret`、`im_doc_url_template`、`im_user_map`。
- **错误码**：不新增（复用 401/403/404/409 既有域错误）。
- **strict 二级**：checked 卡片同样推送（action=approve 走二级）；minimal 档无 review 态自然无卡。

## Review Focus

1. **submit → doc.submitted outbox 行（payload 含 major/actor）；strict checked → doc.checked** → 集成钉
2. **Pusher 消费：im_card_log 落行→卡片送达本地假 IM（JSON 结构含 obj_no/版本/链接）；失败退避→死信 status=2** → core-im 集成钉
3. **钉钉加签签名可复现（timestamp+secret → base64 HMAC）；三渲染器 JSON 契约** → 单元穷举
4. **/api/v1/im/actions 全链：正确签名+映射+有权限 → released；坏签名 401；未映射用户 403；无 docs:approve 403** → e2e 钉
5. **webhook dispatcher 与 IM pusher 双消费者解耦：dispatcher 认领（status=1）后 IM pusher 仍能消费同一事件** → 集成钉（先跑 dispatcher tick 再 pusher tick）

## File Structure

```
crates/core-version/src/store.rs(doc.submitted/doc.checked outbox)
crates/core-im/{Cargo.toml, src/{lib.rs,render.rs(三渲染器+dingtalk 加签),sender.rs(CardSender trait+三实现),pusher.rs(轮询消费)}, migrations/0012_im_card.sql, tests/im_it.rs}
crates/api-gateway/src/routes/im.rs(新端点) + mod.rs(装配 public) + state.rs(im 上下文)
crates/plm-server/src/main.rs(Config 增项+pusher.spawn)
crates/plm-server/tests/e2e.rs(+4 钉)
frontend 无改动（卡片在 IM 端；M 记账）
Cargo.toml(workspace + hmac/base64)
README IM 段；台账
```

## Tasks

### Task 1: doc.submitted/doc.checked 事件（core-version）
- Step1 失败测试：submit→review 落 doc.submitted（payload obj_no/major/actor）；strict 两级第一级落 doc.checked；release/revise 不受影响
- Step2 RED；Step3 实现（released 分支同模式 if new_state=="review"/"checked"）；Step4 GREEN；Step5 Commit

### Task 2: core-im crate（渲染/加签/推送）
- Step1 失败测试：三渲染器 JSON 契约（含 obj_no/版本/url 占位替换）；钉钉加签与已知向量一致；HMAC 校验器 正/反例；Pusher 集成（本地 axum 假 IM 收卡→im_card_log sent；发不出→attempts 退避→死信）；dispatcher 先行认领后 pusher 仍消费（双消费者解耦钉）
- Step2 RED；Step3 实现（0012 迁移+render/sender/pusher）；Step4 GREEN；Step5 Commit

### Task 3: /api/v1/im/actions 端点 + 装配 + e2e
- Step1 失败测试：全链 approve→released（正确签名/映射/enforce 过）；坏签名 401；未映射用户 403；映射用户无 docs:approve 403；越密级 404
- Step2 RED；Step3 实现（routes/im.rs+state.rs+mod.rs public 装配+plm-server Config 增项+pusher.spawn+sender 按 provider）；Step4 GREEN；Step5 Commit

### Task 4: README/台账/全门禁/终审/合并
- Step1 README IM 段（三平台机器人配置+回调契约文档+凭证联调边界声明）；Step2 台账；Step3 全门禁（cargo test workspace+fmt+clippy+vitest+vue-tsc+Playwright 实跑）；Step4 Commit；Step5 fresh reviewer→修复轮→合并 main→push→CI 绿

## Self-Review

覆盖出站三平台+入站闭环+双消费者解耦+退避死信 ↔ Review Focus 5 条 ↔ Task1-4 ✓；签名一致（CardSender trait、im_card_log 语义镜像 dispatcher）✓；迁移号 0012 全局唯一（0011 core-bom 已用）✓；真实凭证边界如实声明（机器人出站真实可用；卡内按钮闭环需企业应用回调网关——契约先行）✓；比例 ✓

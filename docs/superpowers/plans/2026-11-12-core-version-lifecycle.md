# 增量3：core-version 生命周期 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。checkbox 步骤。

**Goal:** docs 对象生命周期闭环——submit→review→(approve→released | reject→working)→obsolete；发布不可变（update 423 已有）+ 发布快照；发布事件进 Outbox（为增量4 core-events 铺底，先落表落库不外投）。

**短设计（Approaches）:** A 表驱动状态机+显式 transition API（选定，概设 §3.2）；B 每处手写 if（违背概设）；C 引入 BPMN（ADR-0004 否决）。权限：submit/reject=docs:write，approve/release/obsolete=docs:approve（admin 专属新权限，editor 无——对齐小微"总工把关"）。快照：release 前整对象（含 attrs）写 cm_obj_snapshot 不可变表。

**Spec:** 概设 §3.2、PRD LCS-01~05、ADR-0004；密级/权限语义沿用已合并实现。

## Global Constraints

- 状态集：working/review/released/obsolete（+扩展位）；非法迁移 → `MDL-LCS-TRANSITION`(409)
- 快照不可变（无 UPDATE/DELETE 路径）；含 full attrs + state + 时点
- 发布事件：topic `doc.released` 落 `event_outbox`（增量4 消费外投；本增量只保证"发布必落事件"）
- 新错误码 `MDL-LCS-TRANSITION`(409)、`MDL-LCS-SNAPSHOT`？不需要（快照失败=Internal）
- 权限：docs:approve 仅 admin（0005 追加种子行——迁移号 0006）

## File Structure

```
crates/core-version/{Cargo.toml, migrations/0006_lifecycle.sql(0007_outbox 后移增量4),
  src/{lib,machine,store.rs}, tests/lifecycle_it.rs}
crates/core-model/src/error.rs(+LCS-TRANSITION)
crates/mod-doc/src/service.rs(+submit/approve/reject/release/obsolete 转发)
crates/api-gateway/src/routes/docs.rs(+5 端点) routes/mod.rs(挂载+perm)
crates/plm-server/tests/{common,e2e}(生命周期链钉子)
```

## Review Focus

1. **released 对象 update → 423**（既有 ObjectStore 行为，生命周期链上再钉）→ e2e
2. **非法迁移**（working→released 跳过审批）→ 409 → 集成钉
3. **reject 回 working 后可再改再提交** → 集成钉
4. **发布快照不可变 + 与活对象隔离**（改快照无路径；活对象后续变更不影响快照内容）→ 集成钉
5. **approve 需 docs:approve**（editor 403）→ e2e 钉

### Task 1: core-version crate（状态机 + 快照 + outbox 表落位）

- [ ] Step1 失败测试 `tests/lifecycle_it.rs`：合法链全通（working→submit→review→approve→released→obsolete）；非法迁移各自 409（working→released、released→submit、obsolete→*）；reject 回 working 后 update 成功；快照在 release 时生成且内容==发布时点（后续 update 活对象不影响快照行）；doc.released 事件行存在于 event_outbox
- [ ] Step2 RED（包不存在）；Step3 实现 machine(纯函数 allowed(state,event))/store(transition+snapshot+outbox 同事务)；Step4 GREEN；Step5 Commit

**Interfaces（Task2/3 消费）:**
- `core_version::migrator()`；`LifecycleEvent { Submit, Approve, Reject, Release, Obsolete }`（serde 小写）
- `LifecycleStore::new(pool) -> Self`；`async transition(&self, object_id: Uuid, ev: LifecycleEvent, actor: Uuid) -> Result<BizObject>`（内含 allowed 校验、released 时快照+outbox）
- `machine::allowed(state, ev) -> Option<State>`（纯函数）

### Task 2: mod-doc 转发 + 网关端点

- [ ] Step1 mod-doc `DocService::{submit,approve,reject,release,obsolete}(obj_no, actor_user)` 薄转发（权限已在网关）；网关 5 端点：`POST /api/v1/docs/{no}/submit|approve|reject|release|obsolete`，perm：write,write,write,approve,approve
- [ ] Step2 编译+既有测试不回归；Commit

### Task 3: e2e 生命周期链 + 门禁 + 合并

- [ ] Step1 e2e：admin 建号 editor→editor 上传+submit→admin approve→released→update 423→obsolete；非法链 409；editor approve 403（Review Focus 全钉）
- [ ] Step2 全门禁（fmt/clippy/54+新测试）；Step3 真机 smoke（登录→提交→审批→发布→快照存在→事件行存在）；Step4 Commit；Step5 终审→修复轮→合并 main

## Self-Review

覆盖 LCS-01/02/04/05（03 并行版本后置）↔ Task1；端点权限↔Task2；Review Focus 5 条↔Task1/3 ✓。类型：LifecycleEvent/LifecycleStore 签名 Task1→2 一致 ✓。比例：~90 行计划 vs ~500 行产出 ✓

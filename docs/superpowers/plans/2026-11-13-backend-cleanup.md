# 增量13：后端杂项收口 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 用户指令"IM 先不做，其他的依次优化"——本增量=deferred 清单后端批。

**Goal:** webhook 死信分页+强校验（增量4 债）、outbox payload 契约统一（actor 补齐）、event_outbox topic 部分索引（增量10 复审备忘）、list_users 搜索（前端搜索框）、migration registry 文档、reject 非对称裁定入档。

**短设计（Approaches）:**
- **A（选定）定点收口，零新概念**：每项按既有模式小改+钉，不做大重构。
- 死信分页：`GET /sec/webhooks/dead?page&size`（缺省 page=1/size=50，上限 200）返回 {items,total,page,size}——仿 docs list 分页语义。
- 强校验：url 须 `http(s)://` 前缀且 host 非空（url::parse 无需新依赖——手动校验前缀+含域名）；secret 长度 ≥ 8（HMAC 密钥强度下限）；topics 每项非空且属于已知主题集（doc.released/doc.revised/doc.submitted/doc.checked）——防拼写静默零投递。
- payload 契约：`doc.released`/`doc.revised` 补 `actor`（提交/升版操作人，subject 一致性对齐 submitted/checked）；键名裁定：released/checked/submitted 用 `major`（版本语义），revised 保留 `old_major/new_major`（升版特有语义对）——写入 migration registry 文档为正式契约。
- 部分索引：0013（core-events）`CREATE INDEX ... ON event_outbox(topic, id) WHERE topic IN ('doc.released','doc.revised','doc.submitted','doc.checked')`——IM 反连接扫描走索引。
- REVOKE 防篡改裁定：**文档化不执行**——当前单连接用户=建表用户，REVOKE 无对象；多角色部署（应用/DBA 分离）时执行，写入 registry 文档。
- reject 非对称裁定：语义=评审行为（UI 按 approve 显隐）；后端 docs:write 允许是 SMB 宽容（editor 兼评审），不改——写入 registry 文档裁定段。

**Spec:** 增量4/8/10/12 台账 deferred 项。

## Global Constraints

- 死信分页：size clamp [1,200]；返回带 created_at/attempts/error（排查需要）。
- 强校验失败 → `MDL-BAD-REQUEST`(400) 带明确 reason；既有合法 webhook 不受影响。
- payload actor：`updated_by.map(|u| u.to_string())` 同 submitted 模式；**存量已发事件不改**（不可变），仅新事件生效。
- 部分索引 `IF NOT EXISTS`；0013 归 core-events（0012 已用——core-im）。
- list_users 加 `search` query（username ILIKE %x%），前端搜索框防抖 300ms。

## Review Focus

1. 死信分页：page/size 边界（size>200 clamp 50、page 越界空集）、含 attempts/error 字段 → e2e 钉
2. 强校验：http:// 前缀缺/secret<8/未知 topic 各 400 + 合法样本 201 → e2e 钉
3. payload 契约：released/revised 含 actor 且为操作人 → 集成钉（更新既有断言）
4. 部分索引存在且 IM 反连接走索引（EXPLAIN 断言可选——至少存在性钉）→ 集成钉
5. list_users search：模糊命中/未命中空集 → e2e 钉

## File Structure

```
crates/api-gateway/src/routes/webhooks.rs(死信分页+强校验)
crates/core-version/src/store.rs(released/revised actor)
crates/core-events/migrations/0013_outbox_topic_idx.sql
crates/api-gateway/src/routes/sec_users.rs(search 参数)
crates/core-sec/src/service.rs(list_users search)
crates/plm-server/tests/e2e.rs(+4 钉) + core-version tests(断言更新)
docs/migrations.md(新：migration registry+payload 契约+裁定段)
frontend/src/pages/UserAdmin.vue(搜索框) + api/sec.ts(search 参数)
```

## Tasks

### Task 1: 死信分页 + webhook 强校验
- Step1 失败 e2e（分页边界/校验三例/合法样本）；Step2 RED；Step3 实现；Step4 GREEN；Step5 Commit

### Task 2: payload 契约统一 + 部分索引
- Step1 失败断言（released/revised actor）；Step2 RED；Step3 实现+0013；Step4 GREEN；Step5 Commit

### Task 3: list_users 搜索 + 前端搜索框 + registry 文档
- Step1 失败 e2e（search 命中/空）；Step2 RED；Step3 实现（后端参数+前端防抖框）；Step4 GREEN；Step5 docs/migrations.md + Commit

### Task 4: README/台账/全门禁/终审/合并/push/CI

## Self-Review

覆盖 deferred 后端批 6 项（IM/REVOKE/where-used 历史化除外——各有裁定或后置）↔ Review Focus 5 ↔ Task1-4 ✓；迁移号 0013 唯一 ✓；存量事件不可变原则 ✓

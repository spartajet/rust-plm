# 增量14：BOM where-used 历史化 + 小项打包 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 常设指令"一直往前做"持续有效；IM 相关暂缓（用户指令）。

**Goal:** 子件的"被谁使用"支持 as-of 历史反查（发布快照反查：件 C 的各 major 曾被用进哪些已发布 BOM、经父件与数量）；详情页补"被谁使用"卡（live + asof 两视图）；顺带 list_users 分页与 webhook 极端分页钉。

**短设计（Approaches）:**
- **A（选定）快照反查聚合**：`bom_edge_snapshot(child=X)` 分组聚合 `(release_root, major, parent) SUM(qty)`——发布快照即历史事实（增量9 不可变裁定直接复用），零新表零迁移；`?asof=1` 返回 NDJSON 行 {release_root, major, parent, qty_total}。
- B（否）live 闭包反查扩展版本维度：增量9 已裁定闭包不挂版本（复杂度爆炸），不翻案。
- live where-used 语义不变（现状反查）。

**Spec:** 增量9 台账（where-used 历史化后置项）、ADR-0003。

## Global Constraints

- `where_used_asof(child)`：`SELECT release_root, major, parent, SUM(qty) FROM bom_edge_snapshot WHERE child=$1 GROUP BY release_root, major, parent ORDER BY release_root, major, parent`。
- 网关：`GET /api/v1/boms/{obj_no}/where-used?asof=1`——asof 行 NDJSON {release_root, major, parent, qty_total}；无 asof 参数=live 现状不变（NDJSON {ancestor→沿用现有字段名}——查现状字段名保持兼容）；ABAC 越级 404 不变。
- 前端 DocDetail"被谁使用"卡：两 tab（当前被用/as-of 发布快照），asof 行显示 root 件号（需 id→obj_no 反查——网关 asof 直接回 parent_no/release_root_no 冗余 obj_no 免前端二次查询——**网关聚合时 JOIN cm_object 回填 obj_no**）。
- list_users 分页：`page/size`（缺省 1/50，上限 200）返回 {items,total,page,size}——**前端 UserAdmin 同步改造**（分页条）。
- 极端分页钉：`/sec/webhooks/dead?page=4294967295&size=200` → 200/空集（M1 防回退）。

## Review Focus

1. where_used_asof 聚合正确：同 child 经多父/多 line_no 聚合；跨发布事件（同 parent 不同 major）分组 → 集成钉（与快照数据对拍）
2. asof 端点 NDJSON 含 release_root_no/parent_no（JOIN 回填）；ABAC 越级 404 → e2e 钉
3. live where-used 字段名兼容（既有 e2e/前端不破）→ 回归钉
4. UserAdmin 分页：翻页 total 一致；搜索+分页组合 → Playwright 钉
5. 极端分页钉 200/空集 → e2e 钉

## File Structure

```
crates/core-bom/src/store.rs(where_used_asof) + tests/bom_it.rs(对拍钉)
crates/api-gateway/src/routes/boms.rs(?asof 分支+obj_no JOIN 回填) + sec_users.rs(分页)
crates/core-sec/src/service.rs(list_users page/size) + tests
crates/plm-server/tests/e2e.rs(+3 钉)
frontend/src/api/{docs.ts(whereUsedAsof), sec.ts(分页)} + pages/{DocDetail.vue(被谁使用卡), UserAdmin.vue(分页条)}
frontend/tests/e2e/flow.spec.ts(+钉)
docs/superpowers/plans/2026-11-13-whereused-asof.md；台账
```

## Tasks

### Task 1: core-bom where_used_asof + 集成钉
- Step1 失败测试：造 A→C 与 B→C 双父 + A 发布/C 发布快照，断言聚合分组与 qty 和；Step2 RED；Step3 实现；Step4 GREEN；Step5 Commit

### Task 2: 网关 asof 端点 + 前端"被谁使用"卡
- Step1 失败 e2e（NDJSON 字段含 obj_no 回填/ABAC 404/live 兼容）；Step2 RED；Step3 实现（网关+前端卡+Playwright 钉）；Step4 GREEN；Step5 Commit

### Task 3: list_users 分页 + 极端分页钉
- Step1 失败 e2e（分页 total/翻页/极端钉）；Step2 RED；Step3 实现（后端+前端分页条+Playwright 翻页钉）；Step4 GREEN；Step5 Commit

### Task 4: README/台账/全门禁/终审/合并/push/CI

## Self-Review

覆盖 where-used 历史化闭环+两小项 ↔ Review Focus 5 ↔ Task1-4 ✓；迁移零新增（复用快照表）✓；字段兼容（live NDJSON 不改键名）✓；比例 ✓

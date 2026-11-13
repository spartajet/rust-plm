# 增量15：live 反查对称过滤 + where-used 卡双视图 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 常设指令"一直往前做"持续有效；IM 相关暂缓（用户指令）。

**Goal:** live where-used 与 as-of 反查语义对称——越密级 ancestor 行级过滤（终审 R2 建议入账项）+ 回填 `ancestor_no`（可读件号）；详情页"被谁使用"卡双视图（当前被用 live / 历史发布快照 asof）；UserAdmin 防抖 timer unmount 清理。

**短设计（Approaches）:**
- **A（选定）镜像 asof 模式**：live 分支 store 纯聚合（where_used 不变）→ 网关批量查 `cm_object(id,obj_no,secret_level)` → 行级密级过滤（ancestor 越级=整行丢弃，存在性隐藏）→ 回填 `ancestor_no`。与增量14 C1 修复完全同构（ABAC 职责归网关、store 纯聚合）。NDJSON 兼容：保留 `ancestor/qty_total` 键，新增 `ancestor_no`。
- B（否）store 内 JOIN 过滤：store 不知查看者密级，破坏纯聚合边界（增量14 M2 已裁定职责分离）。
- 卡双视图：详情页单卡两段（当前被用=live 行 ancestor_no+qty；历史发布快照=asof 行），不设 tab 控件（数据量小，两段上下排布更直白——增量14 I3 裁定的自然延伸）。

**Spec:** 增量14 复审残留 R2（live 对称过滤建议）、增量14 修复轮台账 deferred（防抖清理）。

## Global Constraints

- live NDJSON 行：`{"ancestor": <uuid 兼容保留>, "ancestor_no": <件号>, "qty_total": n}`；越密级 ancestor 整行丢弃（与 asof `root&&parent` 同语义：单维 ancestor 越级即丢）。
- 卡双视图：`当前被用`（live，空态文案"当前未被任何 BOM 使用"）+ `历史发布快照`（asof，空态沿用既有文案）。
- 防抖 timer `onUnmounted` 清理。
- 零迁移。

## Review Focus

1. live 对称过滤：越密级 ancestor 整行丢弃（泄露探针镜像增量14 C1 场景）→ e2e 钉
2. ancestor_no 回填正确（件号而非 UUID）→ e2e 钉
3. 卡双视图渲染（live 段+asof 段共存）→ Playwright 钉
4. 防抖 unmount 清理 → 代码审阅级（Vitest 不便测定时器）
5. 既有消费兼容（ancestor 键保留；bom_it 现有钉不破）→ cargo test 回归

## Tasks

### Task 1: 网关 live 对称过滤 + ancestor_no（e2e 钉）
- Step1 失败 e2e（镜像 C1：越密级 ancestor 过滤+ancestor_no 断言）；Step2 RED；Step3 实现；Step4 GREEN；Step5 Commit

### Task 2: 前端卡双视图 + 防抖清理
- Step1 实现（DocDetail 卡两段+UserAdmin onUnmounted）；Step2 vue-tsc+Vitest；Step3 Playwright 钉（详情页两段渲染）；Step4 Commit

### Task 3: README/台账/全门禁/终审/合并/push/CI

## Self-Review

覆盖 live 对称过滤+双视图+防抖清理 ↔ Review Focus 5 ↔ Task1-3 ✓；模式同构（增量14 C1 镜像）✓；零迁移 ✓；比例 ✓

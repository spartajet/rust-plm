# 增量9：BOM 版本链 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 用户"一直往前做+按 superpowers 流程稳步推进"常设指令下自主决策执行；fresh reviewer 终审为质量门。

**Goal:** released 对象的 BOM 形态按 major 冻结可追溯（发布即快照，同事务原子）；升版后 working v2 编辑不污染已发布 v1 视图；`expand?major=N` 按发布快照 as-of 展开。

**短设计（Approaches）:**
- **A（选定）发布快照冻结 + as-of CTE**：approve 同事务把 live 边集快照进 `bom_edge_snapshot(parent, major, line_no, child, qty)`；live `bom_edge` 语义="最新 major 工作区"（增量8 守卫已保证 released 不可写、revise 后可编辑 v2）；`expand?major=N` 走快照递归 CTE（V-2 实测 10 万边 CTE 全量构建 2.2s / 展开毫秒级，按需查询非瓶颈）。跨 crate 原子性经 `ReleaseHook` 注入：core-version 定义 trait，plm-server 装配 BomReleaseHook（调 core-bom 事务内方法）——core-version 不知 core-bom，依赖方向不破坏（R-D 规则）。
- B（否）bom_edge 挂 parent_major + 闭包挂 (ancestor, ancestor_major)：任意版本走增量闭包更快，但"经父件多级 as-of"需祖先链逐层版本钉扎，增量维护复杂度爆炸——违背 SMB-first。
- C（否）独立 BOM 对象+完整 PDM 版本模型：PRD 远期项，超出本增量纵切。

**Spec:** PRD §6.6 BOM 版本管理、ADR-0003（边表+闭包表）、ADR-0007（事务性事件）、增量8 台账（就地升版裁定/旧版数据先存裁定）。

## Global Constraints

- **快照时机**：`approve → released` 同事务（LifecycleStore 事务内，hook 注入）；快照=快照时点 live 边全量；`ON CONFLICT DO NOTHING` 幂等兜底（同 major 唯一）。
- **live 边语义**：bom_edge 永远只反映最新 major 的当前形态（working 可编辑 / released 423 / revise 后属于新 major）——不再有"未版本化共享边"歧义。
- **as-of 展开**：`expand(obj_no, mode, major: Option<i32>)`；`major=Some(N)`→快照递归 CTE（三模式语义与 live 一致：full=全部后代聚合、leaf=无子边、onelevel=直接一层）；`None`→live bom_path 原路径不变。`N` 无快照 → `MDL-BAD-REQUEST`(400)。
- **where_used 不变**：保持 live 反查语义（历史 where-used 记账后置）。
- **快照守卫**：无 BOM 边的对象 approve → 空快照（不报错）；hook 失败 → 整个迁移回滚（发布不落、事件不发）。
- **API**：`GET /api/v1/boms/{obj_no}/expand?mode=&major=`（major 可选）；ABAC/权限不变。
- **错误码**：不新增（复用 MDL-BAD-REQUEST / 既有错误）。

## Review Focus

1. **approve 后快照存在且形态==发布时点 live 边；hook Err → 迁移整体回滚（状态/事件均不落）** → 集成钉
2. **revise 后 set_lines 改 live 不影响 major=1 as-of；as-of 对拍（同边集下快照 CTE == live expand 三模式）** → 集成钉
3. **expand?major= 无快照 400；ABAC 越级 404 回归** → e2e 钉
4. **全链：v1 发布冻结 → revise → v2 改 BOM → live=新形态 & major=1=冻结形态 → v2 发布 → major=2=新形态** → e2e 钉
5. **无 hook 时 approve 行为与增量8 完全一致（向后兼容）** → 集成钉

## File Structure

```
crates/core-bom/migrations/0011_bom_edge_snapshot.sql
crates/core-bom/src/store.rs(+snapshot_live_edges_tx, +expand_asof 三模式)
crates/core-bom/tests/bom_it.rs(+快照/对拍/隔离钉)
crates/core-version/src/{lib.rs(+ReleaseHook), store.rs(注入+released 分支同事务调用)}
crates/core-version/tests/revise_it.rs(+hook 触发/回滚/无 hook 兼容钉)
crates/api-gateway/src/routes/boms.rs(?major= 接线)
crates/plm-server/{src/main.rs(装配 BomReleaseHook), tests/e2e.rs(全链钉)}
frontend/src/api/docs.ts(bomExpand +major 参数；详情页版本切换后置记账)
docs/superpowers/plans/2026-11-12-bom-version-chain.md
```

## Tasks

### Task 1: core-bom 快照表 + as-of 展开
- Step1 失败测试：0011 迁移后 snapshot_live_edges_tx 落行/幂等/空集；expand_asof 三模式与 live expand 同边集对拍；set_lines 改 live 后 asof 不变（隔离）
- Step2 RED；Step3 实现（0011；snapshot_live_edges_tx(&mut PgConnection)；expand_asof 递归 CTE 三模式）；Step4 GREEN；Step5 Commit

### Task 2: core-version ReleaseHook 注入
- Step1 失败测试：MockHook 收到 (obj_id, major)；hook Err → transition Err 且 cm_object 状态不变/outbox 无行（回滚）；LifecycleStore::new（无 hook）approve 与增量8 行为一致
- Step2 RED；Step3 实现（trait + new_with_hook + released 分支 `&mut **tx` 传递）；Step4 GREEN；Step5 Commit

### Task 3: 网关 ?major= + 装配 BomReleaseHook + e2e 全链
- Step1 失败测试（e2e）：Review Focus #3/#4 全链场景；core-version 集成测试装配真 hook（或 e2e 内断言快照表）
- Step2 RED；Step3 实现（boms.rs ExpandQuery+major 分支；plm-server BomReleaseHook 结构 + LifecycleStore::new_with_hook 装配；前端 docs.ts bomExpand major 可选参数）；Step4 GREEN；Step5 Commit

### Task 4: README + 台账 + 全门禁 + 终审 + 合并
- Step1 README BOM 版本段 + Roadmap 更新；Step2 台账追加；Step3 全门禁（cargo test workspace + fmt + clippy + vitest + vue-tsc + Playwright 实跑）；Step4 Commit；Step5 fresh reviewer 终审→修复轮→合并 main→push→CI 绿确认

## Self-Review

覆盖冻结/隔离/as-of/原子性/向后兼容 ↔ Review Focus 5 条 ↔ Task1-4 ✓；签名一致（&mut PgConnection 事务传递、new 兼容）✓；迁移号 0011 全局唯一（0010 core-version 已用）✓；依赖方向（core-version 定义 hook、core-bom 提供事务内方法、plm-server 装配）✓；比例（后端 3 任务/收尾 1）✓

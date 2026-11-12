# 增量8：生命周期 Revise 升版 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。
> 本计划在用户"一直往前做+按 superpowers 流程稳步推进"常设指令下自主决策执行；fresh reviewer 终审为质量门。

**Goal:** released 对象可升版（Revise）产生下一 major 的 working 副本（就地升版，旧版历史经快照表可追溯）；顺带清偿 M3'（mod-doc 三处硬编码 (1,0) 版本）并封堵 BOM released 态静默污染窗口。

**短设计（Approaches）:**
- **A（选定）就地升版**：同一 cm_object 行 `major_ver++ / minor_ver=0 / state→working`，UUID 身份不变——file_ref、BOM 边、前端路由、webhook 订阅全部零迁移；历史靠 cm_obj_snapshot（approve 时已写 v1.0 released 快照，天然成版本链）。
- B（否）每版本一行：真正行级历史，但 UUID 裂解波及 BOM 边/file_ref/where_used/前端路由全链，且 BOM 边无版本列——留待后续"BOM 版本链"增量整体评估（Revise 是其前置，不是其替代）。
- C（否）revise 语义并入 obsolete+重新 create：丢历史链、丢 obj_no 复用唯一性语义，anti-goal。

**Spec:** PRD §6.5 升版流程、ADR-0004（自研状态机）、ADR-0002（元数据驱动）、概设 §3.4。

## Global Constraints

- **状态机**：`(released, Revise) → working` 三档通用（升版是重入路径，再发布时走各档正常审批链）；working/review/checked/obsolete 上 Revise 非法 → `MDL-LCS-TRANSITION`(409)。obsolete 件不得复活（anti-goal：防僵尸件）。
- **升版事务**（LifecycleStore::transition 内 Revise 特判）：`UPDATE ... SET major_ver=major_ver+1, minor_ver=0, state='working' WHERE id=$1 AND state='released'`——旧状态入 WHERE 守卫（终审 C2 同款），并发双 revise 竞态输家 409；同事务落 `doc.revised` Outbox（payload: type_code/obj_no/old_major/new_major/revised_at）。
- **快照不变式**：revise **不写**新快照（released 快照已在 approve 时写）；快照表新增 `file_hash` 列（迁移 0010），approve 落快照时记录发布件内容哈希——数据先存、旧版文件下载端点后续增量再做（不丢不可重建的历史）。
- **M3' 清偿**：PgObjectStore 新增 `get_latest(type_code, obj_no)`（`ORDER BY major_ver DESC, minor_ver DESC LIMIT 1`，与 transition 取版语义一致）；mod-doc `detail/open_original/update_object` 全部改走 get_latest——升版后 GET /docs/{obj_no} 必须返回 2.0。
- **BOM 守卫**：set_lines 锁 parent 行后（FOR UPDATE 窗口内）校验 `state='working'`，否则 `ObjStateForbidden`(423)——released BOM 不可静默污染；revise 后 working 2.0 可编辑（就地模型下编辑即"草稿 BOM"）。
- **网关**：`POST /api/v1/docs/{obj_no}/revise` 门控 `docs:write`（起草人职责，SMB 敏捷）+ 密级 ABAC 越级 404；`GET /api/v1/docs/{obj_no}/versions` 门控 `docs:read` + ABAC，返回当前版本 + 快照历史（major/minor/state/created_at/file_hash）。
- **前端**：flowUi 增加 `revise` 维度（仅 released 为 true）；详情页显示版本徽章 v{major}.{minor} + 升版按钮 + 版本历史卡；列表加版本列。
- **权限语义**：升版后新 working 版 attrs_rev 沿用行值（就地模型无新行）；会签校验仅在 checked→released 时机触发，revise 后新流程自然重走。

## Review Focus

1. **revise 后：major=2/minor=0/state=working；v1.0 released 快照仍在；outbox 有 doc.revised** → 集成钉
2. **并发双 revise：一胜一 409（不得静默双升）** → 集成钉（两连接对拍）
3. **升版后 detail/open_original/update 走 2.0；乐观锁 attrs_rev 不串版** → 集成钉（M3' 回归）
4. **released/review parent 的 set_lines → 423；working 正常** → 集成钉（BOM 守卫）
5. **revise 需 docs:write（read-only 403）；versions 越级 404 不泄露存在性** → e2e 钉
6. **前端 released→升版按钮→working+v2.0 徽章** → Playwright 钉

## File Structure

```
crates/core-version/src/{machine.rs(+Revise), store.rs(Revise 特判+file_hash)}
crates/core-version/migrations/0010_snapshot_file_hash.sql
crates/core-model/src/store.rs(+get_latest trait/PgObjectStore)
crates/mod-doc/src/{service.rs(get_latest 接线+versions), dto.rs(+VersionHistory)}
crates/core-bom/src/store.rs(set_lines 状态守卫)
crates/api-gateway/src/routes/{docs.rs(revise/versions 端点), mod.rs(装配)}
crates/plm-server/tests/e2e.rs(+钉)
frontend/src/{utils/flowUi.ts(+revise), api/docs.ts(+reviseDoc/listVersions), pages/{DocDetail(+版本徽章/历史卡),DocsList(+版本列)}}
frontend/tests/unit.spec.ts(flowUi 穷举更新)
frontend/tests/e2e/flow.spec.ts(链路延长 revise)
```

## Tasks

### Task 1: Revise 状态机 + 升版事务 + 快照 file_hash（core-version）
- Step1 失败测试：machine 单测（三档 released+revise→working；非法态矩阵）；集成测试 tests/：revise 后 major=2/minor=0/working+快照 v1 仍在+outbox doc.revised+working 上 revise 409+并发双 revise 对拍+approve 快照含 file_hash
- Step2 RED；Step3 实现（machine Revise 分支；store Revise 特判 UPDATE major++/minor=0+doc.revised outbox；transition 加 file_hash 参数；0010 迁移）；Step4 GREEN；Step5 Commit

### Task 2: get_latest + M3' 清偿（core-model/mod-doc）
- Step1 失败集成测试：升版后 get_latest/detail 返回 2.0；update_object 在 2.0 working 成功且 expect_rev 用新行值；open_original 走 2.0
- Step2 RED；Step3 实现（trait+PgObjectStore get_latest；mod-doc detail/open_original/update_object 接线）；Step4 GREEN；Step5 Commit

### Task 3: BOM set_lines 状态守卫（core-bom）
- Step1 失败测试：released/review parent set_lines → 423 ObjStateForbidden；working 正常；锁后查状态（并发 revise 竞态不漏）
- Step2 RED；Step3 实现（事务内 FOR UPDATE 后 SELECT state 校验）；Step4 GREEN；Step5 Commit

### Task 4: 网关端点 revise/versions + mod-doc versions
- Step1 失败测试（plm-server e2e）：POST revise 200→再 409；revise 无 docs:write 403；GET versions 返回当前+历史快照；versions 越级 404；升版后 GET /docs/{obj_no} 返回 2.0（端到端 M3' 钉）
- Step2 RED；Step3 实现（docs.rs lc_endpoint 扩 Revise；versions 端点+DTO；mod-doc::versions；mod.rs 装配 docs_lc_write/docs_r）；Step4 GREEN；Step5 Commit

### Task 5: 前端 flowUi/详情/列表 + Vitest
- Step1 失败 Vitest：flowUi revise 穷举（released→true 其余 false）
- Step2 RED；Step3 实现（flowUi+api/docs.ts reviseDoc/listVersions+DocDetail 版本徽章/升版按钮/历史卡+DocsList 版本列）；Step4 GREEN（vitest+构建通过）；Step5 Commit

### Task 6: Playwright 链路延长 + 全门禁 + 终审 + 合并
- Step1 e2e：released→升版→working+v2.0 徽章断言
- Step2 全门禁（cargo nextest workspace + clippy + fmt + vitest + playwright）；Step3 README 升版段；Step4 Commit；Step5 台账追加；Step6 fresh reviewer 终审→修复轮→合并 main→push origin

## Self-Review

覆盖 Revise 闭环+M3'+BOM 守卫+ABAC/perm+前端 ✓；Review Focus 6 条 ↔ Task1-6 ✓；签名一致（transition 加 file_hash、get_latest、set_lines 不变签名）✓；迁移号 0010 全局唯一（0009 已用）✓；比例（后端 4 任务/前端 1/e2e 1）✓

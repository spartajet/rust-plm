# 增量5：三档流程模板 实施计划（含短设计）

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans（Native，既定）。

**Goal:** SMB-2 落地——对象类型可选流程档位（极简/标准/严格），状态机按档位参数化；档位即配置即时生效；发布语义跨档一致（下游零感知）。

**短设计（Approaches）:** A（选定）**档位=状态机参数**：flow_tier 存 cm_type.config（零新表，TypeRegistry 热生效即档位热生效），machine.rs 三张迁移表；B 独立流程实例表+解释器（=完整 BPMN 引擎，ADR-0004 增量后置）；C 三种类型各配一套状态（类型爆炸，违背单一数据源）。strict 档两级审批含会签校验（checked→released 须异于提交人）。

**Spec:** PRD SMB-2/SMB-5、ADR-0004、概设 §3.4/3.5。

## Global Constraints

- 三档语义：
  - **minimal 极简**：working --submit--> **released**（一键发布留痕；reject 仅 review 态无——即无中间态；released→obsolete 通用）
  - **standard 标准**：现状四态（submit→review→approve→released；reject→working）
  - **strict 严格**：working --submit--> review --approve--> **checked** --approve--> released（两级审批；第二级须**异于提交/复核人**——maker-checker）
- released/obsolete 语义跨档一致：快照、doc.released 事件、423 保护全部不变（"数据无需迁移"）
- 档位读取走 TypeRegistry（热生效，改配置 ≤3s）；缺省 standard
- 会签校验失败 → `MDL-LCS-MAKER-CHECKER`(409)
- 修改档位端点：`PUT /api/v1/meta/types/{code}/flow-tier`（docs:approve 门控）；`GET /api/v1/meta/types/{code}` 读（认证即可）

## Review Focus

1. **minimal submit 直达 released 且快照/事件照发** → 集成钉
2. **strict 两级审批 + 同人第二级 → 409 maker-checker** → 集成钉
3. **档位切换即时生效**（standard→minimal 后 submit 直达）→ 集成钉
4. **跨档 released 对象行为一致**（423/obsolete/快照）→ e2e 钉
5. **档位修改需 docs:approve**（editor 403）→ e2e 钉

## File Structure

```
crates/core-version/src/{machine.rs(参数化), store.rs(读档位+会签), flow.rs(FlowTier)}
crates/core-model/src/error.rs(+MDL-LCS-MAKER-CHECKER)
crates/mod-doc/src/service.rs(transition 带 tier)
crates/api-gateway/src/routes/{meta.rs(新), docs.rs(透传)}
crates/plm-server/tests/e2e.rs(+3 钉)
```

## Tasks

### Task 1: machine 参数化 + FlowTier + 会签校验（core-version/core-model）
- Step1 失败测试 `tests/flow_tiers_it.rs`：三档迁移矩阵（纯函数+集成）；minimal submit→released 且快照/outbox 照发；strict 两级+同人 409 MAKER-CHECKER；档位热切换（改 config→registry.reload→行为翻转）；发布语义跨档一致（快照/outbox/423）
- Step2 RED；Step3 实现（FlowTier::parse/allowed(state,ev,tier)；store 读 type config；checked 态+会签校验用 transition actor vs last_actor）；Step4 GREEN；Step5 Commit

### Task 2: mod-doc/gateway 端点
- Step1 DocService::transition 带 tier（从 registry 读）；网关 meta.rs（GET type / PUT flow-tier，approve 门控）+ docs 5 端点透传（行为不变）；Step2 不回归；Step3 Commit

### Task 3: e2e 三钉 + 门禁 + 终审 + 合并
- Step1 e2e：minimal 链（submit→released 直达+快照+事件）、strict 链（两级+同人 409）、档位修改 403（editor）+切换即时生效
- Step2 全门禁；Step3 README 档位段；Step4 Commit；Step5 终审→修复→合并

## Self-Review
覆盖 SMB-2 最小闭环/Review Focus 5 条 ↔ Task1-3 ✓；签名一致（FlowTier/LifecycleStore.transition 带 tier）✓；比例 ✓

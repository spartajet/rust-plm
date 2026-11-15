# 增量25（TQ-02 OpenAPI 契约快照门禁）终审报告

- 审查人：独立终审 reviewer（fresh eyes）
- 日期：2026-11-16
- 分支：`feat/openapi-snapshot-gate`（基线 main @ 6ef0553）
- 审对象：T1 = 2703327（门禁 e2e + 快照基线），T2 = 9ae3856（文档）
- 方法：git show 两 commit 逐行对账 + 代码审读 + 本机复跑全部验证（含毒化实证三方与 UPDATE 还原字节级比对）

## 总判定：FIX-THEN-MERGE

C1×2（其中一处合并推送后 CI 必红；另一处为 T2 声明失实）。两处修复成本均极低：`cargo fmt` 一键 + 评审报告补一行。修复后无需重审全部项，仅需复核 fmt 与评审报告 diff。

---

## C1（阻塞合并）

### C1-1 `cargo fmt --check` 失败（4 处 diff，全在新增门禁代码）——CI 合并后必红，与基线声明"fmt 过"直接矛盾

- 证据：`cargo fmt --check` 退出码 1，diff 全部位于 `crates/plm-server/tests/e2e.rs:3812 / 3827 / 3840 / 3847`（`extract_face` 内 matches! 行宽、filter_map 折行、`ContractSchema { .. }` 单行结构体、`SNAPSHOT_PATH` concat 本应单行）。
- CI 侧实证：`.github/workflows/ci.yml:39` 为 `cargo fmt --all -- --check`——本次增量自己引入的代码会把自己设计的 CI 门禁打红，"CI 门禁首跑通过"的前提不成立。
- 声明红线：基线声明"fmt 过"与可复现事实相反。
- 最小修复：`cargo fmt`（只动这 4 处，无语义影响），修复后本地 `cargo fmt --check` 退出码 0 即闭环。

### C1-2 T2 声明"评审报告 TQ-02 ⬜→🟡"未落地——commit 未触碰评审报告，TQ-02 仍为 ⬜

- 证据：`git show 9ae3856 --stat` 仅含 README.md、docs/migrations.md、docs/superpowers/plans/2026-11-16-tq02-openapi-gate.md 三文件；`docs/review-需求符合度评审.md` 在 2703327 前后逐字节相同，且现状三处仍为未做：
  - 第 15 行："TQ-02 OpenAPI 契约快照未建"
  - 第 185 行（7.7 TQ 行）："TQ-02 OpenAPI 快照 ⬜"
  - 第 203 行（后置清单）："10. TQ-02 OpenAPI 快照门禁（oasdiff）"
- T2 其余声明全部属实：README"25 个增量"+ 第 4 步更新流 ✅、migrations.md 两裁定 ✅、计划台账回填 ✅——唯独评审报告一项声明与事实不符。
- 最小修复：评审报告补 TQ-02 🟡（7.7 TQ 行 + 第 15 行总评 + 第 203 行清单改"已建，覆盖缺口后置"），并在🟡理由里如实记 I1-1 覆盖缺口（见下）。

## I1（重要，记台账 + 排期）

### I1-1 42 条注册路由中 16 条不在 OpenAPI——门禁对它们整体失明，且"新增路由不注册"也无交叉校验

- 证据：`crates/api-gateway/src/routes/mod.rs` 实注册 42 path（41 个 `/api/v1/*` + `/healthz`）；快照基线 26 path。不在 OpenAPI 的 16 条：docs 生命周期 7 条（approve/reject/release/revise/submit/obsolete/versions）、boms expand、where-used、im/actions、sec/users、sec/roles、角色绑定、sec/webhooks×3。这些端点的删除/改动门禁不感知（与"已知历史缺口 set_lines/expand/where_used"一致，实测缺口比口头认知大得多）。
- 叠加风险：对比只做 spec-vs-基线，无 router-vs-spec 校验——后人新增路由不注册 OpenAPI 时门禁永远绿，契约面会继续缩水。
- migrations.md 现有裁定只记了"类型变更/响应运行时校验不在对比面"，未记此 16 条覆盖缺口。
- 最小修复：本轮先在 migrations.md/评审报告补台账（"OpenAPI 注册面 26/42，其余 16 条后置"）；后续增量把 16 条 handler 补 `#[utoipa::path]` + 注册进 `state.rs` paths(...)，即可被同一门禁接管。

## M1（建议，不阻塞）

- M1-1 请求方向"新增必填"分级语义偏软：给请求 DTO（如 LoginReq）新增 required 字段对老客户端是破坏性变更，但被分级为"快照过期（向后兼容新增）"（e2e.rs:3925-3928）。注：stale 同样触发 panic 拦截（e2e.rs:3944），不会放行，仅消息标签可能误导"确认后更新基线"的决策。建议消息对该情况加一句"请求 schema 新增必填对老客户端破坏，请按破坏性变更评估"。
- M1-2 README.md:146 更新命令 env 与 `cargo` 之间 3 个空格（`postgres   cargo test`，bash 下可运行，纯排版）。
- M1-3 计划文档验收承诺"e2e 新增 2 钉（破坏性拦截/更新模式）"（plans/2026-11-16-tq02-openapi-gate.md:25），实际套件只新增 1 钉（openapi_snapshot_gate）；毒化拦截与 UPDATE 还原为手动实证（本次终审已独立复现，结论成立）。commit message 与台账表述（"实证"）如实，仅计划验收行未回改——建议按事实回填"1 钉 + 手动毒化实证"。

## 已验证通过项（对账成立）

1. **测试基线**：`cargo test --workspace` 171 passed / 0 failed（e2e 38，含 `test openapi_snapshot_gate ... ok`，较增量24 的 37 净增 1）；`cargo clippy --workspace --all-targets` 0 警告。TEST_DATABASE_URL 本机 postgres 真库。
2. **毒化实证三方复现（分级消息正确）**：
   - ghost 必填（DocResponse.required + ghost_field）→ panic 消息归入"破坏性变更：schema DocResponse 必填字段被移除: ghost_field" ✅
   - 删真实 path（batch-approve）+ 加 ghost path 并施 → 单条消息内双分级并列："破坏性变更：端点被删除: /api/v1/ghost-endpoint" + "快照过期：新增端点: /api/v1/docs/batch-approve" ✅
   - UPDATE 模式重写基线后与 git HEAD **字节级一致**（git diff 为空）——既证"update 后未再改契约"，也证 extract_face 确定性（BTreeMap 排序）。
   - 实验后 `git status` clean，基线原样。
3. **D 项核对**：
   - PATCH：`matches!(k.as_str(), "get"|"post"|"put"|"delete"|"patch")`（e2e.rs:3815）——**patch 已在列**，审查前提不成立，代码正确（当前契约面无 delete/patch 端点，属业务现状非代码缺口）。
   - required 缺省空集：基线与活体同一 `extract_face` 生成，两侧对称，"无 required"不会误报 ✅。
   - inline schema（components 外）不覆盖：属实但影响面并入 I1-1（16 条未注册路由的 DTO 本就不在 spec 内）；registered 28 op 的 DTO 均在 components（快照 31 schema 与 state.rs components() 一致）。
   - SNAPSHOT_PATH（e2e.rs:3850-3853）：`env!("CARGO_MANIFEST_DIR")` 为**编译期**固化，CI/本地各自编译时路径自洽；`/` 分隔 Windows 下 std::fs 可用 ✅。
   - EOL：`.gitattributes` `* text=auto eol=lf`，快照 i/lf w/lf；且 serde_json 对 token 间 `\r` 按空白处理，双保险 ✅。
   - UPDATE 写文件单点：唯一 `std::fs::write`（e2e.rs:3870）被 env 门控（3869），正常套件运行只读，与其它 e2e 并发无冲突 ✅。
   - 快照格式：`to_string_pretty + "\n"`，467 行，od 验证以 `}\n` 结尾，key 有序，diff 友好 ✅。
4. **增量24 回扣完好**：template.rs:197 base_type/data_type 白名单校验、:291-294 cod_rule flat→nested 归一、Dockerfile:36 `COPY templates`（include_str 构建态）均在 ✅。
5. **自写对比替代 oasdiff 裁定**：migrations.md 增量25 节两裁定落地 ✅。

## 结论

T1 的门禁实现质量高：结构化契约面设计正确、双向分级实证成立、UPDATE 还原字节级可复现、并发/路径/EOL 边界全部核过。但 C1-1（fmt 失败 → CI 必红 + 基线声明失实）与 C1-2（评审报告声明未落地）触碰对账红线，判 **FIX-THEN-MERGE**：`cargo fmt` + 评审报告补 TQ-02 🟡（含 I1-1 缺口如实记录），修后复核这两点即可合并。

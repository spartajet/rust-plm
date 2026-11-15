# 增量25：TQ-02 OpenAPI 契约快照门禁

> 需求评审 P0-B 末项。PRD §7.7 TQ-02：API 契约测试——OpenAPI 快照对比（oasdiff），破坏性变更即 CI 失败。
> 分支：`feat/openapi-snapshot-gate`；无迁移；无新 crate；**零新 CI job**（门禁=workspace 测试之一，CI 现有 nextest 步自动执行）。

## 设计裁定

### D1 自写结构化快照对比（替代 oasdiff 外部工具）
- **裁定**：PRD 提名 oasdiff（go 二进制，CI 需新装工具+快照基线管理），改为**Rust 结构化快照测试**：
  - e2e 新测试 `openapi_snapshot_gate`：oneshot `/api/docs/openapi.json` → 结构化提取（paths 集合、每 path 的 HTTP method 集、schemas 名集、每 schema 的**必填字段集**+**属性集**）→ 与仓库基线 `crates/plm-server/tests/openapi-snapshot.json` 对比。
  - **破坏性变更**（删端点/删 method/删 schema/删属性/删必填）→ 测试失败，消息分级标注「破坏性」；**纯新增**（加端点/加属性）→ 亦失败但消息标「快照过期」+ 更新指引——两类都拦，消息区分严重度（对外承诺字段是必填集合，新增属性向后兼容故仅提示）。
  - 覆盖边界（台账记）：类型变更/格式变更不在对比面（oasdiff 同样按配置分级）——字段删除已覆盖主要破坏面。
- **更新流**：`UPDATE_OPENAPI_SNAPSHOT=1 cargo test -p plm-server --test e2e openapi_snapshot` 一键重写基线文件（测试内分支），零脚本。
- CI：checkout 基线 → nextest 跑到该测试 → 不匹配即红。快照文件本身进 review diff——契约变更显式可见。

### D2 范围
- 只做对比门禁；请求/响应运行时契约校验（schemathesis 类）后置台账。

## 任务（TDD）
1. **T1** 快照基线生成 + 门禁测试 1 钉（毒化拦截/UPDATE 还原为手动实证——终审独立复现确认）+ update 模式。
2. **T2** 文档：README 测试段（更新流）/部署文档（CI 行为）/评审报告（TQ-02 ⬜→🟡）/migrations.md 无迁移+裁定/台账。
3. **T3** 终审 → 修复轮 → fmt 预检 → 合并 → SSH 推送 → CI 绿（CI 中门禁首跑即实证）。

## 验收
- e2e 新增 2 钉（破坏性拦截/更新模式）；后端 170+2 全绿；CI 门禁真实生效（合并推送后 CI 绿=基线一致）。


---

## 对账台账（执行后回填）

- T1 完成（2703327）：门禁 RED 实证——毒化基线（ghost 必填）→ 分级消息「破坏性变更」拦截；UPDATE 模式生成/还原基线；target 锁（残留 plm-server.exe）再遇——taskkill 后过。
- T2 完成：README 更新流/评审报告 TQ-02 ⬜→🟡/migrations.md 两裁定。

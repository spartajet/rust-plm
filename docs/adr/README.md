# ADR 索引（Architecture Decision Records）

> 本目录记录 RustPLM 的架构决策。一条决策一个文件，编号递增，决策一旦接受不再修改正文（状态变更走 `状态` 字段：提案/已接受/已废弃/被取代）。

## 模板

```markdown
# ADR-NNNN 标题

- 状态：已接受（YYYY-MM-DD）
- 决策人／参与：
- 关联需求：（PRD 编号，如 AGL-01、F1）

## 背景
（为什么需要这个决策：约束、痛点、备选项）

## 决策
（我们决定什么）

## 后果
（正面收益、代价、风险与缓解）
```

## 索引

| 编号 | 标题 | 状态 | 对应概设章节 |
|---|---|---|---|
| [0001](0001-modular-monolith.md) | 单体模块化架构（Modular Monolith，不做微服务） | 已接受 | §2.2 |
| [0002](0002-metadata-model.md) | 元数据驱动对象模型：JSONB + 高频属性提升列 + 在线 DDL | 已接受 | §3.1 |
| [0003](0003-bom-closure-table.md) | BOM 存储：边表 + 闭包表双结构 | 已接受 | §3.3 |
| [0004](0004-self-built-workflow.md) | 工作流引擎自研（轻量状态机 + BPMN 子集） | 已接受 | §3.4 |
| [0005](0005-embedded-tantivy.md) | 全文检索内嵌 tantivy（不引入 Elasticsearch） | 已接受 | §3.5 |
| [0006](0006-wasmtime-plugins.md) | 插件运行时：wasmtime 组件模型 + 能力级授权 | 已接受 | §3.8 |
| [0007](0007-outbox-events.md) | 事件总线：Postgres Outbox + Webhook 投递 | 已接受 | §3.7 |
| [0008](0008-sqlx-postgres.md) | 数据访问：sqlx + PostgreSQL（不用重 ORM） | 已接受 | §2.4 |
| [0009](0009-axum-tokio.md) | 服务端底座：axum + tokio | 已接受 | §2.4 |
| [0010](0010-ai-provider.md) | AI 网关：Provider trait 抽象，云端/本地双形态可断电 | 已接受 | §3.9 |
| [0011](0011-authz.md) | 认证授权：JWT 会话 + casbin RBAC + 自研 ABAC 求值器 | 已接受（2026-11-11 选型） | §3.6 |
| [0012](0012-file-store.md) | 文件库：SHA-256 内容寻址 + 引用计数 GC | 已接受（2026-11-11 选型） | §3.11 |
| [0013](0013-frontend-stack.md) | 前端：Vue3+Element Plus+UnoCSS+vxe-table+bpmn-js+form-create+uni-app | 已接受（2026-11-11 选型） | §2.5 |
| [0014](0014-deployment.md) | 部署：单二进制 + 内置 rustls + Docker Compose 一键 | 已接受（2026-11-11 选型） | §8 |
| [0015](0015-api-style.md) | API：REST + OpenAPI 3.1（utoipa 自动生成） | 已接受（2026-11-11 选型） | §7 |
| [0016](0016-llm-rig.md) | LLM 接入：rig 框架 + 强制中间件包装层 | 已接受（2026-11-11 选型） | §3.9 |
| [0017](0017-dual-license.md) | 开源协议：AGPL-3.0 双许可（≤30 用户免费，超出商业收费） | 已接受（2026-11-11 选型） | 商业模式 |
| [0018](0018-dwg-to-pdf.md) | DWG→PDF：插件端转换起步，服务端转换 V1.5 补 | 已接受（2026-11-11 选型） | 转换服务 |
| [0019](0019-ai-visual-e2e.md) | 开发流程：AI agent 视觉交互自测（开发期实践，非产品能力） | 已接受（2026-11-11 增补/澄清） | 开发工作流 |

> 其他已定基线（PG18 / 无 Redis / thiserror+anyhow / figment / casbin / ClamAV 可选 / plm-cli 备份 / PaddleOCR sidecar / GitHub+Actions / Monorepo / nextest+testcontainers）统一记录于《PLM-技术选型说明书 v1.0》，不单独开 ADR。
>
> 维护规则：每定一条新决策即新增 ADR；正文引用 ADR 编号而非复制内容。

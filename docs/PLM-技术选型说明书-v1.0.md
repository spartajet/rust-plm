# RustPLM 技术选型说明书 v1.0

| 项 | 内容 |
|---|---|
| 日期 | 2026-11-11 |
| 决策方式 | 分 6 批共 23 项交互式决策 + 12 项无争议默认 + 10 项既有 ADR，全部定案 |
| 用途 | 详细设计与开发的**唯一技术基线**；任何变更须走 ADR |
| 配套 | ADR 0001–0018（`docs/adr/`）、概设 §2/§3 |

---

## 1. 选型总表（按领域）

### 1.1 后端核心

| 项 | 选型 | 理由/备注 | 定案 |
|---|---|---|---|
| 语言/运行时 | Rust stable（edition 2024）+ tokio | ADR-0009 | ADR |
| Web 框架 | axum + tower 中间件链 | ADR-0009 | ADR |
| 数据库 | **PostgreSQL 18** | V-1/V-2 已实测验证；支持至 2030+ | 本轮 Q1 |
| 数据访问 | sqlx 0.8（编译期校验+动态双模式） | ADR-0008 | ADR |
| 错误处理 | **thiserror（core 层强类型 DomainError）+ anyhow（应用/CLI 层）** | 错误类型即 API 错误码 | 本轮 Q3 |
| 配置管理 | **figment**（TOML+env+CLI 分层覆盖） | 12-Factor/Docker 场景成熟 | 本轮 Q4 |
| 缓存 | **不引入 Redis**——进程内 moka + PG Outbox/任务表 + 内存令牌桶 | 零中间件（ADR-0001 延伸）；多副本时再评估 | 本轮 Q2 |
| 定时任务 | tokio 生态 + DB cron 表（自研轻量调度，无 Redis 依赖） | 随 Q2 决策推导 | 推导 |
| 序列化 | serde / serde_json | 事实标准 | 默认 |
| 日志/追踪 | tracing + tracing-subscriber（JSON 结构化，OTLP 可选） | 事实标准 | 默认 |

### 1.2 安全

| 项 | 选型 | 理由 | 定案 |
|---|---|---|---|
| 认证 | jsonwebtoken（短效 access + 滚动 refresh）；argon2id 密码；ldap3 对接 AD/LDAP | 概设 §3.6 | 默认 |
| 授权 | **casbin-rs（RBAC）+ 自研 ABAC 五维求值器（<5ms）** | 成熟模型+可控性能 | 本轮 Q5 |
| TLS | **axum 内置 rustls**（单二进制自带 HTTPS、自签自动生成） | 一键部署卖点 | 本轮 Q6 |
| 病毒扫描 | **ClamAV 可选 sidecar**（默认不装，安全敏感客户开启） | 内存与安全平衡 | 本轮 Q7 |
| 备份 | **自研 plm-cli backup**（pg_dump + 文件卷增量 + 哈希校验 + 恢复向导） | 小微百 GB 内足够；pgBackRest 留企业版预案 | 本轮 Q8 |

### 1.3 检索与数据引擎

| 项 | 选型 | 定案 |
|---|---|---|
| 全文检索 | tantivy + jieba-rs（内嵌、按类型分 shard、权限位图过滤） | ADR-0005 |
| 向量/RAG | pgvector + tantivy 混合检索 | ADR-0010 |
| BOM 引擎 | 边表+闭包表（bom_path 含 qty_path，实测展开 42ms/10 万行） | ADR-0003 |
| 元数据 | JSONB+GIN 起步、排序/范围/高基数列提升为物理列 | ADR-0002 + Spike 结论 |

### 1.4 AI 与转换

| 项 | 选型 | 理由/风险 | 定案 |
|---|---|---|---|
| LLM 接入 | **rig 框架** | 多 Provider 抽象/工具调用开箱即用；**注意：脱敏/审计/计量中间件需在 rig Provider 层包装实现**（ADR-0016 记录此约束） | 本轮 Q9 |
| 规则脚本 | **rhai**（沙箱、禁 IO、限循环；AGL-07 业务规则） | 与 Rust 零 FFI；实施人员学习成本低 | 本轮 Q10 |
| DWG→PDF | **插件端转换起步**（CAD 插件调用本机 CAD 批量发布 PDF，保真 100%、零授权费）；服务端转换 V1.5 用 ODA/云 API 补齐 | 避开 ODA 授权费与开源链保真风险；内网客户友好 | 本轮 Q11 |
| OCR | **PaddleOCR sidecar**（独立进程池、模型本地化，满足纯内网模式） | 中文标题栏准确率标杆；作为可选组件安装 | 本轮 Q12 |
| Office→PDF | LibreOffice headless（svc-convert 进程池） | 默认 |

### 1.5 前端

| 项 | 选型 | 理由 | 定案 |
|---|---|---|---|
| 框架+UI | **Vue3 + TypeScript + Element Plus** | 制造业管理端事实标准、中文生态、招人池 | 本轮 Q13 |
| 构建/包管理 | Vite + pnpm workspace | 默认 |
| 状态管理 | Pinia | 默认 |
| CSS | **UnoCSS**（与 EP 共存友好） | 本轮 Q16 |
| 大表格 | **vxe-table**（BOM 10 万行虚拟滚动/可编辑单元格） | 国产 ERP 场景专精 | 本轮 Q14 |
| 流程设计器 | **bpmn-js**（BPMN 2.0 标准，与后端 XML 一致） | 本轮 Q15 |
| 表单设计器 | **form-create**（MIT 开源，二次开发省 3-4 周） | 本轮 Q18 |
| 图表 | Apache ECharts | 默认 |
| 3D 可视化 | **Three.js 自建 viewer**（STEP 经 OCCT 转网格/glTF） | 本轮 Q19 |

### 1.6 移动端与桌面端

| 项 | 选型 | 定案 |
|---|---|---|
| 小程序跨端 | **uni-app**（一套代码 → 微信/钉钉/H5，与 Vue3 同栈复用组件） | 本轮 Q17 |
| 桌面端 | Tauri 2（Rust 核心，<20MB）；CAD 插件（C#/.NET 各家官方 SDK）经本地 HTTP+WebSocket 与桌面端通信 | 概设/默认 |

### 1.7 插件平台

| 项 | 选型 | 定案 |
|---|---|---|
| 后端插件运行时 | wasmtime Component Model（wit 宿主世界、能力授权、AOT） | ADR-0006 |
| 连接器 | Rust 动态库（abi_stable）受信形态；官方 IM/FA/ERP 连接器 dogfood | ADR-0006 |

### 1.8 工程与交付

| 项 | 选型 | 理由 | 定案 |
|---|---|---|---|
| 代码托管 CI | **GitHub + Actions** | 生态最佳、CI 免运维 | 本轮 Q20 |
| 仓库策略 | **Monorepo**（Rust workspace + pnpm workspace + 小程序 + 插件同仓） | 跨栈原子提交、版本对齐 | 本轮 Q21 |
| 测试 | **cargo-nextest + testcontainers（真 PG 18 容器）+ cargo-mutants 抽样**；前端 Vitest；E2E Playwright（开发期辅以 AI agent 视觉自测，ADR-0019） | 本轮 Q22 + 增补 |
| 部署 | 单二进制 + Docker Compose（`docker compose up -d` 一键，镜像 <150MB，amd64/arm64） | 附件 B §6.1 |
| 质量门禁 | rustfmt/clippy/cargo-deny/audit + 性能基准回归（V-1/V-2 程序进 CI） | 已定 |
| **开源协议** | **AGPL-3.0 双许可**：开源版（AGPL-3.0，≤30 用户免费商用含 SaaS）+ 商业许可（>30 用户或需闭源集成/去除 AGPL 义务时购买）；**插件 SDK/连接器 SDK 以 Apache-2.0 发布**（第三方开发者无 copyleft 顾虑，生态优先）；SYS-05 License 计量实现 30 用户闸门 | 本轮 Q23（ADR-0017） |

## 2. AGPL 双许可落地要点（ADR-0017 摘要）

1. **两档授权**：Community（AGPL-3.0，≤30 命名用户，免费商用，SaaS 须遵守 AGPL 开源义务）；Enterprise（商业许可，>30 用户/闭源集成/专属支持，按用户数或模块计费）。
2. **SDK 例外**：`plm-plugin-sdk`、`plm-connector-sdk`、wit 世界定义、REST API 使用均 Apache-2.0——第三方插件/客户端不受传染，保证插件生态（PLG-05）吸引力。
3. **技术闸门**：License 文件（签名计量 30 用户）+ 超限只读降级（不阻断业务数据安全）+ 升级引导。
4. **依赖合规**：Cargo 依赖以 MIT/Apache-2.0 为主，与 AGPL 兼容；CI 加 cargo-deny 许可检查（禁 GPL/LGPL 传染性依赖混入 Apache-2.0 发布的 SDK crate）。
5. **文档义务**：README/LICENSE/DUAL-LICENSING 三文件随包分发；商业条款由商务合同承载。

## 2A. 测试与质量保障体系（对应需求 TQ-01~08，ADR-0019）

### 分层测试金字塔与工具链（产品 CI 门禁）

| 层 | 内容 | 工具 | 运行时机 |
|---|---|---|---|
| L1 单元 | core crate 纯逻辑（状态机/闭包算法/权限求值/编码规则） | cargo-nextest + 内置断言；覆盖率 llvm-cov ≥70% | 每次 PR |
| L2 变异 | 抽样变异测试防"假覆盖" | cargo-mutants（增量抽样） | 每夜 |
| L3 集成 | mod 层对真实 PG 18 的仓储/迁移/事务测试 | testcontainers-rs（每次起独立容器库，隔离零污染） | 每次 PR |
| L4 契约 | API 形状回归 | utoipa 导出 OpenAPI → oasdiff 快照对比 | 每次 PR |
| L5 E2E | 关键旅程全脚本化（S1–S9 逐条映射），复杂交互用拖拽/坐标 API | Playwright（TS）+ 测试数据工厂种子集 | 每夜/发布 |
| L6 视觉回归 | 关键页面像素基线 | Playwright screenshot + 基线对比（阈值容差） | 每夜 |
| L7 性能 | 性能基线回归 | V-1/V-2 Spike 程序固化（BOM 展开 P95、GIN 查询 P95、在线 DDL）+ k6 API 压测 | 每夜/发布 |

### 开发期实践：AI agent 视觉交互自测（★ 非产品能力，ADR-0019）

本项目研发由 AI coding agent 主导，**UI 交互验证采用"agent 驱动浏览器 + 视觉模型判断"的开发期自测闭环**：

```
AI agent 改动前端 ──▶ 启动 dev server / E2E 容器
   ──▶ Playwright（或浏览器自动化 MCP）执行操作并截图
   ──▶ 截图交视觉模型（VLM）判断：布局是否符合预期/组件是否渲染/交互结果是否正确
   ──▶ 不符合 → agent 迭代修复；符合 → 同步沉淀为 L5 Playwright 脚本用例
```

- 定位：**agent 的"眼睛"**——替代人工打开浏览器目检，属开发工作流，不进产品 CI、不承诺为产品功能；
- 价值：把"看起来对不对"前移到编码当下；每轮视觉自测的结论以截图+判断理由归档（PR 描述/会话记录）；
- 收口原则：**视觉自测是过程，Playwright 脚本是资产**——凡自测确认的行为都固化为 L5 用例，防止回归；
- 工具：Playwright 截图 + VLM（模型随开发环境而定，如 GLM-4V/Qwen-VL/GPT-4o，或 MidScene.js 等视觉 agent 框架加速），无厂商绑定。

## 3. 决策记录索引

| 批次 | 问题 | 决策 |
|---|---|---|
| 1 | PG 版本 / Redis / 错误处理 / 配置 | PG18 / 不引入 / thiserror+anyhow / figment |
| 2 | 授权 / TLS / 病毒 / 备份 | casbin+ABAC / 内置 rustls / ClamAV 可选 / plm-cli |
| 3 | LLM / 脚本 / DWG转换 / OCR | **rig** / rhai / 插件端转换起步 / PaddleOCR |
| 4 | 前端栈 / 表格 / 流程设计器 / CSS | Vue3+EP / vxe-table / bpmn-js / UnoCSS |
| 5 | 小程序 / 表单设计器 / 3D | uni-app / form-create / Three.js |
| 6 | 托管CI / 仓库 / 测试 / 协议 | GitHub / Monorepo / nextest+testcontainers / AGPL 双许可 |
| 增补 | 自动化测试体系（L1–L7 金字塔）；**AI agent 视觉交互自测定位为开发期工程实践而非产品能力**（用户澄清 2026-11-11，ADR-0019） |

## 4. 与文档体系的追溯

- 本说明书取代各文档中零散的选型表述；冲突时以本说明书 + ADR 为准。
- 后续动作：ADR 0011–0018 已同步建立（`docs/adr/`）；详细设计按本基线展开（core-model 起步）。

---

*v1.0（2026-11-11）：23 项决策全部定案，技术选型阶段关闭，进入详细设计。*
